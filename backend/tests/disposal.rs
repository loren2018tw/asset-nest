//! 資產報廢整合測試：報廢／復原 API 與 Kea 保留推送（見
//! `.scratch/asset-disposal/spec.md` §4、`docs/adr/0026`）。
//!
//! Kea 測試以最小 stub（僅支援單筆保留推送所需的
//! `reservation-add`／`reservation-del`／`config-write`）複製自
//! `tests/kea_sync.rs`，避免動到既有測試；取捨見票 02 Comments。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::extract::State;
use axum::http::{Method, Request, StatusCode, header};
use axum::routing::post;
use axum::{Json, Router};
use chrono::{Days, Local};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use sqlx::SqlitePool;
use sqlx::sqlite::SqlitePoolOptions;
use tower::ServiceExt;

use asset_nest::kea::http::Client;
use asset_nest::{AppState, app};

// ---------- stub Kea（最小版） ----------

/// stub 中的一筆保留。
#[derive(Debug, Clone, PartialEq, Eq)]
struct StubReservation {
    ip_address: String,
    hw_address: String,
    hostname: Option<String>,
}

/// 記憶體版假 Kea：僅支援單筆保留推送所需命令；`clone` 共用同一份狀態。
#[derive(Clone, Default)]
struct StubKea {
    hosts: Arc<Mutex<HashMap<i64, Vec<StubReservation>>>>,
    fail_commands: Arc<Mutex<Vec<String>>>,
    config_writes: Arc<Mutex<usize>>,
}

impl StubKea {
    /// 啟動 stub 並回傳基底 URL。
    async fn spawn(&self) -> String {
        let router = Router::new()
            .route("/", post(handle))
            .with_state(self.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("綁定 stub 位址");
        let addr = listener.local_addr().expect("stub 位址");
        tokio::spawn(async move {
            axum::serve(listener, router).await.expect("stub 伺服器");
        });
        format!("http://{addr}")
    }

    fn fail_commands(&self, commands: &[&str]) {
        *self.fail_commands.lock().expect("fail") =
            commands.iter().map(|command| command.to_string()).collect();
    }

    fn config_writes(&self) -> usize {
        *self.config_writes.lock().expect("config_writes")
    }

    fn hosts(&self, subnet_id: i64) -> Vec<StubReservation> {
        self.hosts
            .lock()
            .expect("hosts")
            .get(&subnet_id)
            .cloned()
            .unwrap_or_default()
    }
}

/// stub 的命令處理：回應格式比照 Kea（單元素陣列、result 0／1／3）。
async fn handle(State(stub): State<StubKea>, Json(payload): Json<Value>) -> Json<Value> {
    let command = payload
        .get("command")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();

    if stub
        .fail_commands
        .lock()
        .expect("fail")
        .iter()
        .any(|failed| failed == &command)
    {
        return Json(json!([{ "result": 1, "text": "simulated failure" }]));
    }

    let arguments = payload.get("arguments").cloned().unwrap_or(Value::Null);
    let response = match command.as_str() {
        "reservation-add" => {
            let reservation = &arguments["reservation"];
            let subnet_id = reservation["subnet-id"].as_i64().unwrap_or(0);
            let entry = StubReservation {
                ip_address: reservation["ip-address"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                hw_address: reservation["hw-address"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                hostname: reservation
                    .get("hostname")
                    .and_then(Value::as_str)
                    .map(str::to_string),
            };
            let mut hosts = stub.hosts.lock().expect("hosts");
            let list = hosts.entry(subnet_id).or_default();
            list.retain(|host| host.ip_address != entry.ip_address);
            list.push(entry);
            json!([{ "result": 0, "text": "Host added." }])
        }
        "reservation-del" => {
            let subnet_id = arguments["subnet-id"].as_i64().unwrap_or(0);
            let ip = arguments["ip-address"].as_str().unwrap_or_default();
            let mut hosts = stub.hosts.lock().expect("hosts");
            let list = hosts.entry(subnet_id).or_default();
            let before = list.len();
            list.retain(|host| host.ip_address != ip);
            if before != list.len() {
                json!([{ "result": 0, "text": "Host deleted." }])
            } else {
                json!([{ "result": 3, "text": "Host not found." }])
            }
        }
        "config-write" => {
            *stub.config_writes.lock().expect("config_writes") += 1;
            json!([{ "result": 0, "text": "Configuration written." }])
        }
        other => json!([{ "result": 2, "text": format!("unsupported: {other}") }]),
    };

    Json(response)
}

// ---------- 測試輔助 ----------

/// 建立測試資料庫並套用 migrations（比照 `delete_linkage.rs`）。
async fn test_pool() -> SqlitePool {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("建立記憶體資料庫");

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("套用 migrations");

    pool
}

fn dist_dir() -> PathBuf {
    std::env::temp_dir().join("asset-nest-test-no-dist")
}

/// 建立不附 Kea 的 AppState（一般流程測試）。
fn plain_state(pool: &SqlitePool) -> AppState {
    AppState::new(pool.clone(), dist_dir())
}

/// 建立附掛 stub Kea 的 AppState。
async fn kea_state() -> (AppState, StubKea) {
    let stub = StubKea::default();
    let url = stub.spawn().await;
    let state = AppState::new(test_pool().await, dist_dir())
        .with_kea(Client::new(url.parse().expect("stub URL")));
    (state, stub)
}

/// 以 `oneshot` 發送請求；回傳狀態碼與 JSON（空內容為 `Value::Null`）。
async fn send(
    state: &AppState,
    method: Method,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(uri);
    let body = match body {
        Some(value) => {
            builder = builder.header(header::CONTENT_TYPE, "application/json");
            Body::from(value.to_string())
        }
        None => Body::empty(),
    };

    let response = app(state.clone())
        .oneshot(builder.body(body).expect("建立請求"))
        .await
        .expect("執行請求");

    let status = response.status();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("讀取回應內容")
        .to_bytes();

    let json = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).expect("回應為 JSON")
    };

    (status, json)
}

/// 將查詢值編碼為 URI 可接受的百分比格式（比照 `tests/assets.rs`）。
fn encode(value: &str) -> String {
    value
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                char::from(byte).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

/// 今天（本地時區）的 `YYYY-MM-DD`。
fn today() -> String {
    Local::now().date_naive().format("%Y-%m-%d").to_string()
}

/// 新增資產並斷言成功，回傳 id。
async fn create_asset(state: &AppState, body: Value) -> i64 {
    let (status, json) = send(state, Method::POST, "/api/v1/assets", Some(body)).await;
    assert_eq!(status, StatusCode::CREATED, "新增資產應成功：{json}");
    json["id"].as_i64().expect("回應含 id")
}

/// 對資產新增介面並斷言成功，回傳 id。
async fn create_interface(state: &AppState, asset_id: i64, body: Value) -> i64 {
    let (status, json) = send(
        state,
        Method::POST,
        &format!("/api/v1/assets/{asset_id}/interfaces"),
        Some(body),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "新增介面應成功：{json}");
    json["id"].as_i64().expect("回應含 id")
}

/// 新增網段並斷言成功，回傳 id。
async fn create_subnet(state: &AppState, body: Value) -> i64 {
    let (status, json) = send(state, Method::POST, "/api/v1/subnets", Some(body)).await;
    assert_eq!(status, StatusCode::CREATED, "新增網段應成功：{json}");
    json["id"].as_i64().expect("回應含 id")
}

/// 指派／改用途（不檢查狀態碼，供各測試自行斷言）。
async fn put_assignment(
    state: &AppState,
    subnet_id: i64,
    address: &str,
    body: Value,
) -> (StatusCode, Value) {
    send(
        state,
        Method::PUT,
        &format!("/api/v1/subnets/{subnet_id}/ips/{address}/assignment"),
        Some(body),
    )
    .await
}

/// 報廢（不檢查狀態碼，供各測試自行斷言）。
async fn dispose(state: &AppState, asset_id: i64, body: Value) -> (StatusCode, Value) {
    send(
        state,
        Method::POST,
        &format!("/api/v1/assets/{asset_id}/disposal"),
        Some(body),
    )
    .await
}

/// 復原（不檢查狀態碼，供各測試自行斷言）。
async fn restore(state: &AppState, disposed_id: i64) -> (StatusCode, Value) {
    send(
        state,
        Method::POST,
        &format!("/api/v1/disposed-assets/{disposed_id}/restore"),
        None,
    )
    .await
}

/// 讀取報廢清單（`query` 含 `?`；空字串＝無參數）。
async fn list_disposed(state: &AppState, query: &str) -> (StatusCode, Value) {
    send(
        state,
        Method::GET,
        &format!("/api/v1/disposed-assets{query}"),
        None,
    )
    .await
}

/// 目前資產清單的全部 id（預設排序）。
async fn asset_ids(state: &AppState) -> Vec<i64> {
    let (status, page) = send(state, Method::GET, "/api/v1/assets?per_page=100", None).await;
    assert_eq!(status, StatusCode::OK, "讀取資產清單應成功：{page}");
    page["items"]
        .as_array()
        .expect("items 為陣列")
        .iter()
        .map(|item| item["id"].as_i64().expect("回應含 id"))
        .collect()
}

/// 建立借出（不檢查狀態碼）。
async fn create_lending(state: &AppState, asset_id: i64, body: Value) -> (StatusCode, Value) {
    send(
        state,
        Method::POST,
        &format!("/api/v1/assets/{asset_id}/lendings"),
        Some(body),
    )
    .await
}

/// 歸還借出紀錄（不檢查狀態碼）。
async fn return_lending(state: &AppState, lending_id: i64) -> (StatusCode, Value) {
    send(
        state,
        Method::POST,
        &format!("/api/v1/lendings/{lending_id}/return"),
        None,
    )
    .await
}

/// 直接查資料庫：某資產的借還紀錄筆數。
async fn lending_count(pool: &SqlitePool, asset_id: i64) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM lendings WHERE asset_id = ?")
        .bind(asset_id)
        .fetch_one(pool)
        .await
        .expect("查詢借還紀錄")
}

// ---------- 流程 ----------

#[tokio::test]
async fn disposal_flow_moves_asset_and_restore_returns_it() {
    let pool = test_pool().await;
    let state = plain_state(&pool);

    // 全欄位資產＋介面＋指派（static）。
    let asset_id = create_asset(
        &state,
        json!({
            "property_no": "P-001",
            "description": "測試筆電",
            "location": "機房 A",
            "device_serial": "SN-123",
            "brand": "Dell",
            "model": "Latitude 5540",
            "purchase_date": "2024-01-15",
            "lifespan_years": 5,
            "note": "測試備註",
            "tags": ["標籤A", "標籤B"],
        }),
    )
    .await;
    let interface_id = create_interface(
        &state,
        asset_id,
        json!({ "name": "eth0", "mac": "aa:bb:cc:dd:ee:01" }),
    )
    .await;
    let subnet_id = create_subnet(&state, json!({ "cidr": "10.0.0.0/29" })).await;
    let (status, body) = put_assignment(
        &state,
        subnet_id,
        "10.0.0.1",
        json!({ "interface_id": interface_id, "purpose": "static" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "指派應成功：{body}");

    // 報廢 201：報廢欄位攤平、未涉 Kea 同步（空陣列）。
    let date = today();
    let (status, body) = dispose(&state, asset_id, json!({ "disposal_date": date })).await;
    assert_eq!(status, StatusCode::CREATED, "報廢應成功：{body}");
    let disposed_id = body["id"].as_i64().expect("回應含報廢列 id");
    assert_eq!(body["asset_id"], asset_id, "保留原資產 id");
    assert_eq!(body["property_no"], "P-001");
    assert_eq!(body["description"], "測試筆電");
    assert_eq!(body["location"], "機房 A");
    assert_eq!(body["device_serial"], "SN-123");
    assert_eq!(body["brand"], "Dell");
    assert_eq!(body["model"], "Latitude 5540");
    assert_eq!(body["purchase_date"], "2024-01-15");
    assert_eq!(body["lifespan_years"], 5);
    assert_eq!(body["note"], "測試備註");
    assert_eq!(body["tags"], json!(["標籤A", "標籤B"]));
    assert_eq!(body["disposal_date"], date);
    assert_eq!(body["kea_sync"], json!([]), "未受管網段不涉同步");

    // 資產 404、清單不含。
    let (status, _) = send(
        &state,
        Method::GET,
        &format!("/api/v1/assets/{asset_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "報廢後資產詳情 404");
    assert!(
        !asset_ids(&state).await.contains(&asset_id),
        "資產清單不含報廢資產"
    );

    // 報廢清單含該筆（欄位與報廢日期）。
    let (status, page) = list_disposed(&state, "").await;
    assert_eq!(status, StatusCode::OK, "讀取報廢清單應成功：{page}");
    assert_eq!(page["total"], 1);
    assert_eq!(page["items"][0]["id"], disposed_id);
    assert_eq!(page["items"][0]["asset_id"], asset_id);
    assert_eq!(page["items"][0]["description"], "測試筆電");
    assert_eq!(page["items"][0]["disposal_date"], date);

    // 原 IP 可重新指派給另一資產（介面已連動刪除）。
    let other_asset = create_asset(
        &state,
        json!({ "description": "另一台", "location": "機房 B" }),
    )
    .await;
    let other_interface = create_interface(
        &state,
        other_asset,
        json!({ "name": "eth0", "mac": "aa:bb:cc:dd:ee:02" }),
    )
    .await;
    let (status, body) = put_assignment(
        &state,
        subnet_id,
        "10.0.0.1",
        json!({ "interface_id": other_interface, "purpose": "static" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "原 IP 應可重新指派：{body}");

    // 復原 200：原 id、無介面／指派。
    let (status, body) = restore(&state, disposed_id).await;
    assert_eq!(status, StatusCode::OK, "復原應成功：{body}");
    assert_eq!(body["id"], asset_id, "復原沿用原 id");
    assert_eq!(body["description"], "測試筆電");
    assert_eq!(body["tags"], json!(["標籤A", "標籤B"]));
    let (status, detail) = send(
        &state,
        Method::GET,
        &format!("/api/v1/assets/{asset_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "復原後資產詳情可讀");
    assert_eq!(detail["interfaces"], json!([]), "介面不回復");
    assert_eq!(detail["assignments"], json!([]), "指派不回復");

    // 清單互換。
    let (status, page) = list_disposed(&state, "").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(page["total"], 0, "復原後報廢清單不含");
    assert!(
        asset_ids(&state).await.contains(&asset_id),
        "資產清單重新出現"
    );
}

// ---------- 驗證 ----------

#[tokio::test]
async fn disposal_validation_rejects_missing_bad_and_future_dates() {
    let pool = test_pool().await;
    let state = plain_state(&pool);
    let asset_id = create_asset(
        &state,
        json!({ "description": "驗證主機", "location": "機房 A" }),
    )
    .await;

    // 缺日期（含空字串）：必填。
    for body in [json!({}), json!({ "disposal_date": "  " })] {
        let (status, body) = dispose(&state, asset_id, body).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "缺日期應 400：{body}");
        assert_eq!(body["error"], "validation_error");
        assert_eq!(body["details"]["field"], "disposal_date");
        assert_eq!(body["message"], "報廢日期為必填");
    }

    // 非嚴格 `YYYY-MM-DD`。
    let (status, body) = dispose(&state, asset_id, json!({ "disposal_date": "2026/1/1" })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["details"]["field"], "disposal_date");
    assert_eq!(body["message"], "報廢日期須為有效日期（YYYY-MM-DD）");

    // 未來日期。
    let tomorrow = (Local::now().date_naive() + Days::new(1))
        .format("%Y-%m-%d")
        .to_string();
    let (status, body) = dispose(&state, asset_id, json!({ "disposal_date": tomorrow })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["details"]["field"], "disposal_date");
    assert_eq!(body["message"], "報廢日期不可為未來日期");

    // 驗證失敗不搬移。
    let (status, _) = send(
        &state,
        Method::GET,
        &format!("/api/v1/assets/{asset_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "驗證失敗不應搬移");

    // 今天可。
    let (status, body) = dispose(&state, asset_id, json!({ "disposal_date": today() })).await;
    assert_eq!(status, StatusCode::CREATED, "今天可報廢：{body}");
}

// ---------- 出借中 ----------

#[tokio::test]
async fn disposal_blocked_while_lent_and_cascades_lending_history() {
    let pool = test_pool().await;
    let state = plain_state(&pool);
    let asset_id = create_asset(
        &state,
        json!({ "description": "借出主機", "location": "機房 A" }),
    )
    .await;

    // 已歸還歷史＋出借中各一筆。
    let (status, history) = create_lending(&state, asset_id, json!({ "borrower": "王小明" })).await;
    assert_eq!(status, StatusCode::CREATED, "建立借出應成功：{history}");
    let history_id = history["id"].as_i64().expect("回應含 id");
    let (status, _) = return_lending(&state, history_id).await;
    assert_eq!(status, StatusCode::OK);
    let (status, open) = create_lending(&state, asset_id, json!({ "borrower": "陳大頭" })).await;
    assert_eq!(status, StatusCode::CREATED);
    let open_id = open["id"].as_i64().expect("回應含 id");

    // 出借中 → 409；資產與紀錄仍在。
    let (status, body) = dispose(&state, asset_id, json!({ "disposal_date": today() })).await;
    assert_eq!(status, StatusCode::CONFLICT, "出借中不可報廢：{body}");
    assert_eq!(body["error"], "conflict");
    assert_eq!(body["message"], "此資產出借中，請先歸還");
    let (status, _) = send(
        &state,
        Method::GET,
        &format!("/api/v1/assets/{asset_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "被擋下時不搬移");
    assert_eq!(lending_count(&pool, asset_id).await, 2);

    // 歸還後可報廢；借還紀錄（含已歸還歷史）連動刪除。
    let (status, _) = return_lending(&state, open_id).await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = dispose(&state, asset_id, json!({ "disposal_date": today() })).await;
    assert_eq!(status, StatusCode::CREATED, "歸還後可報廢：{body}");
    assert_eq!(
        lending_count(&pool, asset_id).await,
        0,
        "借還紀錄全數連動刪除（含已歸還歷史）"
    );
}

// ---------- 復原 ----------

#[tokio::test]
async fn restore_missing_and_repeat_return_not_found() {
    let pool = test_pool().await;
    let state = plain_state(&pool);

    // 不存在。
    let (status, body) = restore(&state, 9999).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"], "not_found");
    assert_eq!(body["message"], "找不到報廢資產");

    // 復原一次成功；重複復原 404。
    let asset_id = create_asset(
        &state,
        json!({ "description": "復原主機", "location": "機房 A" }),
    )
    .await;
    let (status, body) = dispose(&state, asset_id, json!({ "disposal_date": today() })).await;
    assert_eq!(status, StatusCode::CREATED);
    let disposed_id = body["id"].as_i64().expect("回應含報廢列 id");

    let (status, _) = restore(&state, disposed_id).await;
    assert_eq!(status, StatusCode::OK, "首次復原應成功");

    let (status, body) = restore(&state, disposed_id).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "重複復原應 404");
    assert_eq!(body["error"], "not_found");
    assert_eq!(body["message"], "找不到報廢資產");
}

// ---------- 清單 ----------

#[tokio::test]
async fn list_searches_and_clamps_pagination() {
    let pool = test_pool().await;
    let state = plain_state(&pool);

    // 三筆報廢：報廢日期新→舊。
    let entries = [
        ("P-100", "一般筆電", "機房 A", "2026-10-01"),
        ("P-200", "測試桌機", "機房 B", "2026-10-02"),
        ("P-300", "備用筆電", "機房 C", "2026-10-03"),
    ];
    for (property_no, description, location, date) in entries {
        let asset_id = create_asset(
            &state,
            json!({
                "property_no": property_no,
                "description": description,
                "location": location,
            }),
        )
        .await;
        let (status, body) = dispose(&state, asset_id, json!({ "disposal_date": date })).await;
        assert_eq!(status, StatusCode::CREATED, "報廢應成功：{body}");
    }

    // 預設分頁與排序。
    let (status, page) = list_disposed(&state, "").await;
    assert_eq!(status, StatusCode::OK, "讀取報廢清單應成功：{page}");
    assert_eq!(page["total"], 3);
    assert_eq!(page["page"], 1);
    assert_eq!(page["per_page"], 10, "per_page 預設 10");
    let items = page["items"].as_array().expect("items 為陣列");
    assert_eq!(items[0]["disposal_date"], "2026-10-03");
    assert_eq!(items[2]["disposal_date"], "2026-10-01");

    // q 關鍵字搜尋（描述子字串）。
    let (status, page) = list_disposed(&state, &format!("?q={}", encode("筆電"))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(page["total"], 2);
    let descriptions: Vec<&str> = page["items"]
        .as_array()
        .expect("items 為陣列")
        .iter()
        .filter_map(|item| item["description"].as_str())
        .collect();
    assert!(
        descriptions.contains(&"一般筆電") && descriptions.contains(&"備用筆電"),
        "q 命中筆電：{descriptions:?}"
    );

    // page／per_page。
    let (status, page) = list_disposed(&state, "?page=2&per_page=2").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(page["total"], 3);
    assert_eq!(page["page"], 2);
    assert_eq!(page["per_page"], 2);
    assert_eq!(
        page["items"].as_array().expect("items 為陣列").len(),
        1,
        "第二頁剩 1 筆"
    );
    assert_eq!(page["items"][0]["disposal_date"], "2026-10-01");

    // 夾範圍：page 由 1、per_page 1..=100。
    let (status, page) = list_disposed(&state, "?page=0&per_page=0").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(page["page"], 1, "page 下限 1");
    assert_eq!(page["per_page"], 1, "per_page 下限 1");
    let (status, page) = list_disposed(&state, "?per_page=1000").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(page["per_page"], 100, "per_page 上限 100");
}

// ---------- Kea ----------

#[tokio::test]
async fn disposal_pushes_reservation_deletes_to_kea() {
    let (state, stub) = kea_state().await;
    let subnet = create_subnet(&state, json!({ "cidr": "10.0.0.0/24", "kea_subnet_id": 1 })).await;
    let asset_id = create_asset(
        &state,
        json!({ "description": "保留主機", "location": "機房 A" }),
    )
    .await;
    let first = create_interface(
        &state,
        asset_id,
        json!({ "name": "eth0", "mac": "aa:bb:cc:dd:ee:01" }),
    )
    .await;
    let second = create_interface(
        &state,
        asset_id,
        json!({ "name": "eth1", "mac": "aa:bb:cc:dd:ee:02" }),
    )
    .await;
    let third = create_interface(
        &state,
        asset_id,
        json!({ "name": "eth2", "mac": "aa:bb:cc:dd:ee:03" }),
    )
    .await;

    // 同網段兩筆保留（不同介面）＋一筆 static（不涉同步）。
    for (address, interface_id, hostname) in [
        ("10.0.0.5", first, Some("pc-5")),
        ("10.0.0.6", second, Some("pc-6")),
    ] {
        let (status, body) = put_assignment(
            &state,
            subnet,
            address,
            json!({ "interface_id": interface_id, "purpose": "reservation", "hostname": hostname }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{address} 指派應成功：{body}");
    }
    let (status, body) = put_assignment(
        &state,
        subnet,
        "10.0.0.7",
        json!({ "interface_id": third, "purpose": "static" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "static 指派應成功：{body}");
    assert_eq!(stub.hosts(1).len(), 2, "stub 有兩筆保留");

    // 報廢：逐筆推送刪除、KeaSync ok、stub 保留消失。
    let writes_before = stub.config_writes();
    let (status, body) = dispose(&state, asset_id, json!({ "disposal_date": today() })).await;
    assert_eq!(status, StatusCode::CREATED, "報廢應成功：{body}");
    let syncs = body["kea_sync"].as_array().expect("kea_sync 為陣列");
    assert_eq!(syncs.len(), 2, "僅保留逐筆推送：{body}");
    assert!(
        syncs.iter().all(|sync| sync["status"] == "ok"),
        "推送結果為 ok：{body}"
    );
    assert!(stub.hosts(1).is_empty(), "報廢後 stub 保留消失");
    assert_eq!(
        stub.config_writes() - writes_before,
        2,
        "逐筆刪除各寫入一次 Kea 設定檔"
    );

    // 本地已搬移。
    let (status, _) = send(
        &state,
        Method::GET,
        &format!("/api/v1/assets/{asset_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, page) = list_disposed(&state, "").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(page["total"], 1);
}

#[tokio::test]
async fn disposal_kea_failure_still_disposes_and_reports_failed() {
    let (state, stub) = kea_state().await;
    let subnet = create_subnet(&state, json!({ "cidr": "10.0.0.0/24", "kea_subnet_id": 1 })).await;
    let asset_id = create_asset(
        &state,
        json!({ "description": "失敗主機", "location": "機房 A" }),
    )
    .await;
    let interface_id = create_interface(
        &state,
        asset_id,
        json!({ "name": "eth0", "mac": "aa:bb:cc:dd:ee:01" }),
    )
    .await;
    let (status, body) = put_assignment(
        &state,
        subnet,
        "10.0.0.5",
        json!({ "interface_id": interface_id, "purpose": "reservation", "hostname": "pc-5" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "指派應成功：{body}");
    assert_eq!(stub.hosts(1).len(), 1);

    // stub 失敗：仍 201、kea_sync failed、本地已搬移（見 ADR-0011）。
    stub.fail_commands(&["reservation-del"]);
    let writes_before = stub.config_writes();
    let (status, body) = dispose(&state, asset_id, json!({ "disposal_date": today() })).await;
    assert_eq!(status, StatusCode::CREATED, "Kea 失敗仍應報廢：{body}");
    assert_eq!(body["kea_sync"][0]["status"], "failed");
    assert!(
        body["kea_sync"][0]["message"]
            .as_str()
            .is_some_and(|message| !message.is_empty()),
        "應帶失敗訊息：{body}"
    );
    assert_eq!(stub.config_writes(), writes_before, "失敗時不寫設定檔");
    assert_eq!(stub.hosts(1).len(), 1, "Kea 保留仍在，待完整同步修復");

    let (status, _) = send(
        &state,
        Method::GET,
        &format!("/api/v1/assets/{asset_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "本地已搬移");
    let (status, page) = list_disposed(&state, "").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(page["total"], 1, "報廢清單含該筆");
}
