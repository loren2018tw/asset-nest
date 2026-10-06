//! 資產管理整合測試：CRUD、必填驗證、搜尋與篩選、屆齡計算（見票 01）、
//! 標籤、標籤篩選、伺服器端排序與 `GET /tags`（見票 11）、清單已指派 IP
//! 欄與 MAC／IP 搜尋（見票 12）、資產匯出（見票 04）。

use axum::body::Body;
use axum::http::{HeaderMap, Method, Request, StatusCode, header};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use sqlx::SqlitePool;
use sqlx::sqlite::SqlitePoolOptions;
use tower::ServiceExt;

use asset_nest::{AppState, app};

/// 建立測試資料庫並套用 migrations（骨架測試未套；領域測試必須先套）。
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

/// 以 `oneshot` 發送請求；回傳狀態碼與 JSON（204 等空內容為 `Value::Null`）。
async fn send(
    pool: &SqlitePool,
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

    let response = app(AppState::new(
        pool.clone(),
        std::env::temp_dir().join("asset-nest-test-no-dist"),
    ))
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

/// 以 `oneshot` 發送 GET；回傳狀態碼、標頭與原始內容（CSV 等非 JSON 回應用）。
async fn send_bytes(pool: &SqlitePool, uri: &str) -> (StatusCode, HeaderMap, Vec<u8>) {
    let request = Request::builder()
        .method(Method::GET)
        .uri(uri)
        .body(Body::empty())
        .expect("建立請求");

    let response = app(AppState::new(
        pool.clone(),
        std::env::temp_dir().join("asset-nest-test-no-dist"),
    ))
    .oneshot(request)
    .await
    .expect("執行請求");

    let status = response.status();
    let headers = response.headers().clone();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("讀取回應內容")
        .to_bytes()
        .to_vec();

    (status, headers, bytes)
}

/// 發送資產匯出請求；`params` 為查詢字串（不含 `?`，可空）。
async fn export_bytes(pool: &SqlitePool, params: &str) -> (StatusCode, HeaderMap, Vec<u8>) {
    let uri = if params.is_empty() {
        "/api/v1/assets/export".to_string()
    } else {
        format!("/api/v1/assets/export?{params}")
    };
    send_bytes(pool, &uri).await
}

/// 解析匯出 CSV（跳過 BOM）；回傳標題與資料列。
fn parse_export(bytes: &[u8]) -> (Vec<String>, Vec<csv::StringRecord>) {
    assert_eq!(&bytes[..3], &[0xEF, 0xBB, 0xBF], "回應須有 UTF-8 BOM");
    let mut reader = csv::Reader::from_reader(&bytes[3..]);
    let headers = reader
        .headers()
        .expect("標題列")
        .iter()
        .map(str::to_string)
        .collect();
    let rows = reader
        .into_records()
        .map(|record| record.expect("資料列"))
        .collect();
    (headers, rows)
}

/// 新增資產並斷言成功，回傳回應 JSON。
async fn create_asset(pool: &SqlitePool, body: Value) -> Value {
    let (status, json) = send(pool, Method::POST, "/api/v1/assets", Some(body)).await;
    assert_eq!(status, StatusCode::CREATED, "新增資產應成功：{json}");
    json
}

/// 對資產新增介面並斷言成功，回傳 id。
async fn create_interface(pool: &SqlitePool, asset_id: i64, body: Value) -> i64 {
    let (status, json) = send(
        pool,
        Method::POST,
        &format!("/api/v1/assets/{asset_id}/interfaces"),
        Some(body),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "新增介面應成功：{json}");
    json["id"].as_i64().expect("回應含 id")
}

/// 新增網段並斷言成功，回傳 id。
async fn create_subnet(pool: &SqlitePool, body: Value) -> i64 {
    let (status, json) = send(pool, Method::POST, "/api/v1/subnets", Some(body)).await;
    assert_eq!(status, StatusCode::CREATED, "新增網段應成功：{json}");
    json["id"].as_i64().expect("回應含 id")
}

/// 指派位址（可指定用途與 hostname）並斷言成功。
async fn assign(
    pool: &SqlitePool,
    subnet_id: i64,
    address: &str,
    interface_id: i64,
    purpose: &str,
    hostname: Option<&str>,
) {
    let mut body = json!({ "interface_id": interface_id, "purpose": purpose });
    if let Some(hostname) = hostname {
        body["hostname"] = json!(hostname);
    }

    let (status, json) = send(
        pool,
        Method::PUT,
        &format!("/api/v1/subnets/{subnet_id}/ips/{address}/assignment"),
        Some(body),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "指派 {address} 應成功：{json}");
}

/// 將位址以手動設定（static）指派給介面並斷言成功。
async fn assign_static(pool: &SqlitePool, subnet_id: i64, address: &str, interface_id: i64) {
    assign(pool, subnet_id, address, interface_id, "static", None).await;
}

/// 植入一筆觀測現況（比照 `observation_history.rs`；見票 08）。
async fn insert_presence(
    pool: &SqlitePool,
    subnet_id: i64,
    address: &str,
    last_seen_at: Option<&str>,
    mac: Option<&str>,
    source: Option<&str>,
    checked_at: Option<&str>,
) {
    sqlx::query(
        "INSERT INTO ip_presence
             (subnet_id, address, last_seen_at, last_seen_mac, last_seen_source, last_checked_at)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(subnet_id)
    .bind(address)
    .bind(last_seen_at)
    .bind(mac)
    .bind(source)
    .bind(checked_at)
    .execute(pool)
    .await
    .expect("植入觀測現況");
}

/// 讀取清單中某資產的「最後可見」欄（JSON null 映射為 `None`）。
async fn last_seen_of(pool: &SqlitePool, asset_id: i64) -> Option<String> {
    let (status, page) = send(pool, Method::GET, "/api/v1/assets?per_page=200", None).await;
    assert_eq!(status, StatusCode::OK);
    let value = page["items"]
        .as_array()
        .expect("items 為陣列")
        .iter()
        .find(|item| item["id"].as_i64() == Some(asset_id))
        .map(|item| item["last_seen_at"].clone())
        .expect("清單含該資產");
    if value.is_null() {
        None
    } else {
        Some(value.as_str().expect("最後可見為字串").to_string())
    }
}

/// 讀取單一資產詳情的「最後可見」欄。
async fn detail_last_seen_of(pool: &SqlitePool, asset_id: i64) -> Option<String> {
    let (status, detail) = send(
        pool,
        Method::GET,
        &format!("/api/v1/assets/{asset_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    if detail["last_seen_at"].is_null() {
        None
    } else {
        Some(
            detail["last_seen_at"]
                .as_str()
                .expect("最後可見為字串")
                .to_string(),
        )
    }
}

/// 以 `q` 搜尋並回傳命中的資產 id（依預設排序）。
async fn search_ids(pool: &SqlitePool, q: &str) -> Vec<i64> {
    let (status, page) = send(
        pool,
        Method::GET,
        &format!("/api/v1/assets?q={}", encode(q)),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "q={q}");
    page["items"]
        .as_array()
        .expect("items 為陣列")
        .iter()
        .map(|item| item["id"].as_i64().expect("回應含 id"))
        .collect()
}

/// 讀取清單中某資產的「已指派 IP」欄。
async fn assigned_ips_of(pool: &SqlitePool, asset_id: i64) -> Vec<String> {
    let (status, page) = send(pool, Method::GET, "/api/v1/assets?per_page=200", None).await;
    assert_eq!(status, StatusCode::OK);
    page["items"]
        .as_array()
        .expect("items 為陣列")
        .iter()
        .find(|item| item["id"].as_i64() == Some(asset_id))
        .and_then(|item| item["assigned_ips"].as_array())
        .expect("清單列含 assigned_ips 陣列")
        .iter()
        .map(|value| value.as_str().expect("位址為字串").to_string())
        .collect()
}

/// 讀取清單中某資產的「出借中摘要」欄（JSON null 映射為 `None`；見票 02）。
async fn lending_of(pool: &SqlitePool, asset_id: i64) -> Option<Value> {
    let (status, page) = send(pool, Method::GET, "/api/v1/assets?per_page=200", None).await;
    assert_eq!(status, StatusCode::OK);
    let value = page["items"]
        .as_array()
        .expect("items 為陣列")
        .iter()
        .find(|item| item["id"].as_i64() == Some(asset_id))
        .map(|item| item["lending"].clone())
        .expect("清單含該資產");
    if value.is_null() { None } else { Some(value) }
}

/// 建立借出並斷言成功，回傳借出紀錄 id（見票 02）。
async fn create_lending(pool: &SqlitePool, asset_id: i64, body: Value) -> i64 {
    let (status, json) = send(
        pool,
        Method::POST,
        &format!("/api/v1/assets/{asset_id}/lendings"),
        Some(body),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "建立借出應成功：{json}");
    json["id"].as_i64().expect("回應含 id")
}

/// 歸還借出紀錄並斷言成功（見票 02）。
async fn return_lending(pool: &SqlitePool, lending_id: i64) {
    let (status, json) = send(
        pool,
        Method::POST,
        &format!("/api/v1/lendings/{lending_id}/return"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "歸還應成功：{json}");
}

/// 將查詢值編碼為 URI 可接受的百分比格式（僅處理測試用到的字元）。
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

#[tokio::test]
async fn create_read_update_delete_asset() {
    let pool = test_pool().await;

    let (status, created) = send(
        &pool,
        Method::POST,
        "/api/v1/assets",
        Some(json!({
            "property_no": "A-001",
            "description": "測試伺服器",
            "location": "機房 A",
            "device_serial": "SN-001",
            "brand": "Dell",
            "model": "R740",
            "purchase_date": "2000-01-01",
            "lifespan_years": 5,
            "note": "測試用"
        })),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED);
    let id = created["id"].as_i64().expect("回應含 id");
    assert!(id > 0, "id 為資料庫自增");
    assert_eq!(created["description"], "測試伺服器");
    assert_eq!(created["location"], "機房 A");
    assert_eq!(created["brand"], "Dell");
    assert_eq!(created["tags"], json!([]), "未提供標籤時為空陣列");
    assert_eq!(created["expired"], true, "2000 年購置＋5 年已屆齡");
    assert!(
        created["created_at"]
            .as_str()
            .is_some_and(|v| !v.is_empty()),
        "回應含建立時間"
    );

    let (status, fetched) = send(&pool, Method::GET, &format!("/api/v1/assets/{id}"), None).await;
    assert_eq!(status, StatusCode::OK);
    // 詳情在資產欄位之外另含 interfaces（票 02 擴充，見 spec §5）
    assert_eq!(
        fetched["description"], created["description"],
        "讀回內容與新增一致"
    );
    assert_eq!(fetched["created_at"], created["created_at"]);
    assert_eq!(fetched["interfaces"], json!([]), "新增資產尚無介面");

    let (status, updated) = send(
        &pool,
        Method::PATCH,
        &format!("/api/v1/assets/{id}"),
        Some(json!({
            "description": "測試伺服器（改名）",
            "location": "機房 B",
            "purchase_date": "2020-01-01",
            "lifespan_years": 50
        })),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["description"], "測試伺服器（改名）");
    assert_eq!(updated["location"], "機房 B");
    assert_eq!(updated["expired"], false, "未逾年限");
    assert_eq!(updated["property_no"], "A-001", "未提供的欄位維持原值");
    assert_eq!(updated["brand"], "Dell", "未提供的欄位維持原值");

    let (status, cleared) = send(
        &pool,
        Method::PATCH,
        &format!("/api/v1/assets/{id}"),
        Some(json!({ "brand": null, "note": "  " })),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert!(cleared["brand"].is_null(), "顯式 null 清除選填欄位");
    assert!(cleared["note"].is_null(), "空白字串視為清除");
    assert_eq!(cleared["description"], "測試伺服器（改名）");

    let (status, body) = send(&pool, Method::DELETE, &format!("/api/v1/assets/{id}"), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(body, Value::Null);

    let (status, body) = send(&pool, Method::GET, &format!("/api/v1/assets/{id}"), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"], "not_found");

    let (status, _) = send(&pool, Method::DELETE, &format!("/api/v1/assets/{id}"), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "刪除不存在的資產回 404");

    // id 單調遞增（AUTOINCREMENT）
    let second = create_asset(
        &pool,
        json!({ "description": "第二台", "location": "機房 A" }),
    )
    .await;
    assert!(
        second["id"].as_i64().expect("回應含 id") > id,
        "id 為資料庫自增且不重用"
    );
}

#[tokio::test]
async fn create_requires_description_and_location() {
    let pool = test_pool().await;

    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/v1/assets",
        Some(json!({ "brand": "Dell" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "validation_error");
    assert_eq!(body["message"], "描述為必填");
    assert_eq!(body["details"]["field"], "description");

    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/v1/assets",
        Some(json!({ "description": "   " })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["message"], "描述為必填", "全空白視為缺漏");

    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/v1/assets",
        Some(json!({ "description": "電腦" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["message"], "位置為必填");
    assert_eq!(body["details"]["field"], "location");

    // 選填欄位的結構錯誤也必須擋下（見 ADR-0006）
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/v1/assets",
        Some(json!({
            "description": "電腦",
            "location": "機房",
            "purchase_date": "2020/01/01"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["details"]["field"], "purchase_date");

    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/v1/assets",
        Some(json!({
            "description": "電腦",
            "location": "機房",
            "lifespan_years": -1
        })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["details"]["field"], "lifespan_years");

    // 編輯時的必填驗證同樣阻擋
    let asset = create_asset(&pool, json!({ "description": "電腦", "location": "機房" })).await;
    let id = asset["id"].as_i64().expect("回應含 id");
    let (status, body) = send(
        &pool,
        Method::PATCH,
        &format!("/api/v1/assets/{id}"),
        Some(json!({ "location": "" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["message"], "位置為必填");

    // 驗證失敗不寫入任何資料
    let (_, page) = send(&pool, Method::GET, "/api/v1/assets", None).await;
    assert_eq!(page["total"], 1, "僅存在一筆成功建立的資產");
}

#[tokio::test]
async fn list_searches_and_filters_server_side() {
    let pool = test_pool().await;

    create_asset(
        &pool,
        json!({
            "property_no": "PC-001",
            "description": "會計部電腦",
            "location": "總公司 3F",
            "device_serial": "SN-AAA",
            "brand": "Lenovo",
            "model": "ThinkCentre",
            "note": "財務用"
        }),
    )
    .await;
    create_asset(
        &pool,
        json!({
            "description": "機房交換器",
            "location": "機房 A",
            "device_serial": "SW-XYZ",
            "brand": "Cisco",
            "model": "C9300",
            "note": "核心交換器"
        }),
    )
    .await;
    create_asset(
        &pool,
        json!({
            "description": "櫃檯印表機",
            "location": "總公司 3F",
            "brand": "HP",
            "model": "LaserJet",
            "note": "彩色"
        }),
    )
    .await;

    // 關鍵字跨 財產編號／描述／設備序號／廠牌／型號／備註；子字串、不分大小寫
    let cases = [
        ("PC-001", "會計部電腦"),
        ("會計", "會計部電腦"),
        ("SN-AAA", "會計部電腦"),
        ("thinkcentre", "會計部電腦"),
        ("財務", "會計部電腦"),
        ("c9300", "機房交換器"),
        ("彩色", "櫃檯印表機"),
    ];
    for (q, expected) in cases {
        let (status, page) = send(&pool, Method::GET, &format!("/api/v1/assets?q={q}"), None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(page["total"], 1, "q={q} 應命中 1 筆");
        assert_eq!(page["items"][0]["description"], expected, "q={q}");
    }

    // 關鍵字含 LIKE 萬用字元時視為字面字元
    let (_, page) = send(&pool, Method::GET, "/api/v1/assets?q=%25", None).await;
    assert_eq!(page["total"], 0, "% 不應變成萬用字元");

    // 位置與廠牌篩選：不分大小寫完全符合
    let (status, page) = send(
        &pool,
        Method::GET,
        &format!("/api/v1/assets?location={}", encode("總公司 3F")),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(page["total"], 2);

    let (_, page) = send(
        &pool,
        Method::GET,
        &format!("/api/v1/assets?location={}", encode("機房 a")),
        None,
    )
    .await;
    assert_eq!(page["total"], 1, "位置篩選不分大小寫");
    assert_eq!(page["items"][0]["description"], "機房交換器");

    let (_, page) = send(&pool, Method::GET, "/api/v1/assets?brand=hp", None).await;
    assert_eq!(page["total"], 1);
    assert_eq!(page["items"][0]["description"], "櫃檯印表機");

    // 關鍵字＋篩選可組合
    let (_, page) = send(
        &pool,
        Method::GET,
        &format!(
            "/api/v1/assets?q={}&location={}",
            encode("電腦"),
            encode("總公司 3F")
        ),
        None,
    )
    .await;
    assert_eq!(page["total"], 1);
    assert_eq!(page["items"][0]["description"], "會計部電腦");
}

#[tokio::test]
async fn list_paginates_and_orders_by_description() {
    let pool = test_pool().await;
    for description in ["Bravo", "alpha", "Charlie"] {
        create_asset(
            &pool,
            json!({ "description": description, "location": "機房" }),
        )
        .await;
    }

    let (status, page) = send(&pool, Method::GET, "/api/v1/assets?per_page=2&page=1", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(page["total"], 3);
    assert_eq!(page["page"], 1);
    assert_eq!(page["per_page"], 2);
    assert_eq!(page["items"].as_array().expect("items 為陣列").len(), 2);

    // 預設排序：描述升冪（不分大小寫）
    assert_eq!(page["items"][0]["description"], "alpha");
    assert_eq!(page["items"][1]["description"], "Bravo");

    let (_, page) = send(&pool, Method::GET, "/api/v1/assets?per_page=2&page=2", None).await;
    assert_eq!(page["items"].as_array().expect("items 為陣列").len(), 1);
    assert_eq!(page["items"][0]["description"], "Charlie");

    // 預設每頁 50 筆（spec §6）
    let (_, page) = send(&pool, Method::GET, "/api/v1/assets", None).await;
    assert_eq!(page["per_page"], 50);
}

#[tokio::test]
async fn locations_and_brands_are_deduplicated_case_insensitively() {
    let pool = test_pool().await;

    for (description, location, brand) in [
        ("一", "機房 A", "Dell"),
        ("二", "MDF", "HP"),
        ("三", "mdf", "hp"),
        ("四", " 機房 A ", "DELL"),
    ] {
        create_asset(
            &pool,
            json!({ "description": description, "location": location, "brand": brand }),
        )
        .await;
    }

    let (status, body) = send(&pool, Method::GET, "/api/v1/locations", None).await;
    assert_eq!(status, StatusCode::OK);
    let locations: Vec<&str> = body["items"]
        .as_array()
        .expect("items 為陣列")
        .iter()
        .map(|value| value.as_str().expect("位置為字串"))
        .collect();
    assert_eq!(
        locations,
        vec!["MDF", "機房 A"],
        "不分大小寫去重，保留最早寫入的原文"
    );

    let (_, body) = send(&pool, Method::GET, "/api/v1/brands", None).await;
    let brands: Vec<&str> = body["items"]
        .as_array()
        .expect("items 為陣列")
        .iter()
        .map(|value| value.as_str().expect("廠牌為字串"))
        .collect();
    assert_eq!(brands, vec!["Dell", "HP"]);
}

#[tokio::test]
async fn duplicate_device_serial_is_only_a_hint() {
    let pool = test_pool().await;

    create_asset(
        &pool,
        json!({ "description": "設備一", "location": "機房", "device_serial": "SN-001" }),
    )
    .await;
    let second = create_asset(
        &pool,
        json!({ "description": "設備二", "location": "機房", "device_serial": "sn-001" }),
    )
    .await;
    assert!(
        second["id"].as_i64().is_some_and(|id| id > 0),
        "重複設備序號不阻擋建立"
    );

    // 精確查找（前端據此顯示提示，不阻擋）
    let (_, page) = send(
        &pool,
        Method::GET,
        "/api/v1/assets?device_serial=SN-001",
        None,
    )
    .await;
    assert_eq!(page["total"], 2, "不分大小寫的精確查找");
}

#[tokio::test]
async fn expired_flag_is_computed_by_server() {
    use chrono::{Duration, Local};

    let pool = test_pool().await;
    let today = Local::now().date_naive();
    let format_date = |date: chrono::NaiveDate| date.format("%Y-%m-%d").to_string();

    let cases = [
        // （購置日期, 年限, 期望屆齡）
        (Some(today - Duration::days(4000)), Some(10), true),
        (Some(today), Some(0), false),
        (Some(today - Duration::days(1)), Some(0), true),
        (Some(today), None, false),
        (None, Some(10), false),
    ];

    for (index, (purchase_date, lifespan_years, expected)) in cases.into_iter().enumerate() {
        let body = json!({
            "description": format!("資產 {index}"),
            "location": "機房",
            "purchase_date": purchase_date.map(format_date),
            "lifespan_years": lifespan_years,
        });
        let created = create_asset(&pool, body).await;
        assert_eq!(
            created["expired"], expected,
            "purchase_date={:?}, lifespan_years={:?}",
            purchase_date, lifespan_years
        );
    }

    // 屆齡僅提示，不影響任何操作
    let (_, page) = send(&pool, Method::GET, "/api/v1/assets", None).await;
    let expired_id = page["items"]
        .as_array()
        .expect("items 為陣列")
        .iter()
        .find(|item| item["expired"] == true)
        .and_then(|item| item["id"].as_i64())
        .expect("清單中有屆齡資產");

    let (status, updated) = send(
        &pool,
        Method::PATCH,
        &format!("/api/v1/assets/{expired_id}"),
        Some(json!({ "note": "屆齡仍可編輯" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["note"], "屆齡仍可編輯");

    let (status, _) = send(
        &pool,
        Method::DELETE,
        &format!("/api/v1/assets/{expired_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "屆齡仍可刪除");
}

#[tokio::test]
async fn tags_are_normalized_on_create_and_patch() {
    let pool = test_pool().await;

    // 建立：trim、忽略空字串、不分大小寫去重（保留首次出現原樣）
    let created = create_asset(
        &pool,
        json!({
            "description": "有標籤的資產",
            "location": "機房",
            "tags": [" 核心 ", "Core", "core", "", "   ", "備援"]
        }),
    )
    .await;
    assert_eq!(created["tags"], json!(["核心", "Core", "備援"]));

    let id = created["id"].as_i64().expect("回應含 id");

    // 詳情含 tags
    let (status, fetched) = send(&pool, Method::GET, &format!("/api/v1/assets/{id}"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(fetched["tags"], json!(["核心", "Core", "備援"]));

    // 未提供 tags＝維持原值
    let (status, updated) = send(
        &pool,
        Method::PATCH,
        &format!("/api/v1/assets/{id}"),
        Some(json!({ "note": "只改備註" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["tags"], json!(["核心", "Core", "備援"]));

    // 提供陣列＝設定新值（同樣正規化）
    let (status, updated) = send(
        &pool,
        Method::PATCH,
        &format!("/api/v1/assets/{id}"),
        Some(json!({ "tags": [" A ", "a", "B", "b", "B"] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["tags"], json!(["A", "B"]));

    // 顯式 null＝清空
    let (status, cleared) = send(
        &pool,
        Method::PATCH,
        &format!("/api/v1/assets/{id}"),
        Some(json!({ "tags": null })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cleared["tags"], json!([]));

    // 建立時未提供 tags＝空陣列
    let bare = create_asset(
        &pool,
        json!({ "description": "無標籤", "location": "機房" }),
    )
    .await;
    assert_eq!(bare["tags"], json!([]));
}

#[tokio::test]
async fn tag_filter_matches_exactly_and_case_insensitively() {
    let pool = test_pool().await;

    create_asset(
        &pool,
        json!({
            "description": "資料庫主機",
            "location": "機房",
            "tags": ["Prod", "DB"]
        }),
    )
    .await;
    create_asset(
        &pool,
        json!({
            "description": "測試主機",
            "location": "機房",
            "tags": ["staging"]
        }),
    )
    .await;
    create_asset(
        &pool,
        json!({ "description": "備份主機", "location": "機房" }),
    )
    .await;

    // 不分大小寫完全符合；子字串不命中
    for (tag, expected) in [
        ("prod", 1),
        ("PROD", 1),
        ("db", 1),
        ("Staging", 1),
        ("pro", 0),
        ("missing", 0),
    ] {
        let (status, page) = send(
            &pool,
            Method::GET,
            &format!("/api/v1/assets?tag={tag}"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(page["total"], expected, "tag={tag}");
    }

    let (_, page) = send(&pool, Method::GET, "/api/v1/assets?tag=prod", None).await;
    assert_eq!(page["items"][0]["description"], "資料庫主機");

    // 與其他篩選可組合
    let (_, page) = send(
        &pool,
        Method::GET,
        &format!("/api/v1/assets?tag=prod&location={}", encode("機房")),
        None,
    )
    .await;
    assert_eq!(page["total"], 1);
}

/// 依查詢字串讀取清單並回傳描述順序。
async fn descriptions(pool: &SqlitePool, query: &str) -> Vec<String> {
    let (status, page) = send(pool, Method::GET, &format!("/api/v1/assets?{query}"), None).await;
    assert_eq!(status, StatusCode::OK, "{query}");
    page["items"]
        .as_array()
        .expect("items 為陣列")
        .iter()
        .map(|item| {
            item["description"]
                .as_str()
                .expect("描述為字串")
                .to_string()
        })
        .collect()
}

#[tokio::test]
async fn list_sorts_by_whitelisted_columns_with_direction() {
    let pool = test_pool().await;

    // A：屆齡、標籤 ["b"]；B：未屆齡、標籤 ["a"]；C：無選填值、無標籤
    create_asset(
        &pool,
        json!({
            "property_no": "PC-2",
            "description": "Bravo",
            "location": "機房 B",
            "brand": "Zeta",
            "model": "M2",
            "note": "n2",
            "purchase_date": "2000-01-01",
            "lifespan_years": 1,
            "tags": ["b"]
        }),
    )
    .await;
    create_asset(
        &pool,
        json!({
            "description": "alpha",
            "location": "機房 A",
            "brand": "Alpha",
            "model": "M1",
            "note": "n1",
            "tags": ["a"]
        }),
    )
    .await;
    create_asset(
        &pool,
        json!({ "property_no": "PC-1", "description": "Charlie", "location": "機房 A" }),
    )
    .await;

    // 預設：描述升冪（不分大小寫）
    assert_eq!(
        descriptions(&pool, "per_page=10").await,
        vec!["alpha", "Bravo", "Charlie"]
    );

    // 文字欄位升／降冪；NULL 依 SQLite 預設（升冪在前、降冪在後）
    let cases = [
        (
            "sort=property_no&dir=asc",
            vec!["alpha", "Charlie", "Bravo"],
        ),
        (
            "sort=property_no&dir=desc",
            vec!["Bravo", "Charlie", "alpha"],
        ),
        (
            "sort=description&dir=asc",
            vec!["alpha", "Bravo", "Charlie"],
        ),
        (
            "sort=description&dir=desc",
            vec!["Charlie", "Bravo", "alpha"],
        ),
        ("sort=location&dir=asc", vec!["alpha", "Charlie", "Bravo"]),
        ("sort=brand&dir=asc", vec!["Charlie", "alpha", "Bravo"]),
        ("sort=brand&dir=desc", vec!["Bravo", "alpha", "Charlie"]),
        ("sort=model&dir=asc", vec!["Charlie", "alpha", "Bravo"]),
        ("sort=note&dir=desc", vec!["Bravo", "alpha", "Charlie"]),
        // tags 以 JSON 字串排序：["a"] < ["b"] < []
        ("sort=tags&dir=asc", vec!["alpha", "Bravo", "Charlie"]),
        ("sort=tags&dir=desc", vec!["Charlie", "Bravo", "alpha"]),
        // expired：升冪未屆齡在前（同值以 id 決勝）
        ("sort=expired&dir=asc", vec!["alpha", "Charlie", "Bravo"]),
        ("sort=expired&dir=desc", vec!["Bravo", "alpha", "Charlie"]),
    ];
    for (query, expected) in cases {
        assert_eq!(descriptions(&pool, query).await, expected, "{query}");
    }
}

#[tokio::test]
async fn list_sorts_by_first_assigned_ip_with_unassigned_last() {
    let pool = test_pool().await;

    let subnet4 = create_subnet(&pool, json!({ "cidr": "10.0.0.0/24" })).await;
    let subnet6 = create_subnet(&pool, json!({ "cidr": "fd00::/64" })).await;

    // alpha：10.0.0.10（文字序會排在 10.0.0.9 之前，須以數值序）
    let alpha = create_asset(&pool, json!({ "description": "alpha", "location": "機房" })).await;
    let alpha_id = alpha["id"].as_i64().expect("回應含 id");
    let alpha_interface = create_interface(&pool, alpha_id, json!({ "name": "eth0" })).await;
    assign_static(&pool, subnet4, "10.0.0.10", alpha_interface).await;

    // Bravo：10.0.0.9
    let bravo = create_asset(&pool, json!({ "description": "Bravo", "location": "機房" })).await;
    let bravo_id = bravo["id"].as_i64().expect("回應含 id");
    let bravo_interface = create_interface(&pool, bravo_id, json!({ "name": "eth0" })).await;
    assign_static(&pool, subnet4, "10.0.0.9", bravo_interface).await;

    // Charlie：未指派（排序固定最後，且供篩選組合驗證）
    create_asset(
        &pool,
        json!({ "description": "Charlie", "location": "機房 B" }),
    )
    .await;

    // Delta：僅 v6 → 第一筆為 v6，排在所有 v4 之後
    let delta = create_asset(&pool, json!({ "description": "Delta", "location": "機房" })).await;
    let delta_id = delta["id"].as_i64().expect("回應含 id");
    let delta_interface = create_interface(&pool, delta_id, json!({ "name": "eth0" })).await;
    assign_static(&pool, subnet6, "fd00::5", delta_interface).await;

    // Echo：先指派 v6 再指派 v4 → 第一筆仍取顯示序的 v4
    let echo = create_asset(&pool, json!({ "description": "Echo", "location": "機房" })).await;
    let echo_id = echo["id"].as_i64().expect("回應含 id");
    let echo_interface = create_interface(&pool, echo_id, json!({ "name": "eth0" })).await;
    assign_static(&pool, subnet6, "fd00::9", echo_interface).await;
    assign_static(&pool, subnet4, "10.0.0.2", echo_interface).await;

    // 升冪：第一筆位址數值序；v4 全在 v6 前；未指派最後
    assert_eq!(
        descriptions(&pool, "sort=assigned_ips&dir=asc&per_page=10").await,
        vec!["Echo", "Bravo", "alpha", "Delta", "Charlie"]
    );
    // 降冪：升冪的完全反向（v6 排在 v4 前），未指派仍最後
    assert_eq!(
        descriptions(&pool, "sort=assigned_ips&dir=desc&per_page=10").await,
        vec!["Delta", "alpha", "Bravo", "Echo", "Charlie"]
    );

    // 分頁：以排序後的完整序列切頁，總數不變
    let (status, page) = send(
        &pool,
        Method::GET,
        "/api/v1/assets?sort=assigned_ips&dir=asc&per_page=2&page=2",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(page["total"], 5);
    let page_descriptions: Vec<&str> = page["items"]
        .as_array()
        .expect("items 為陣列")
        .iter()
        .map(|item| item["description"].as_str().expect("描述為字串"))
        .collect();
    assert_eq!(page_descriptions, ["alpha", "Delta"]);

    // 可與既有篩選組合
    assert_eq!(
        descriptions(
            &pool,
            &format!(
                "sort=assigned_ips&per_page=10&location={}",
                encode("機房 B")
            )
        )
        .await,
        vec!["Charlie"]
    );

    // 匯出沿用同一排序
    let (status, _, bytes) = export_bytes(&pool, "sort=assigned_ips&dir=asc").await;
    assert_eq!(status, StatusCode::OK);
    let (_, rows) = parse_export(&bytes);
    let exported: Vec<&str> = rows.iter().map(|row| row.get(1).expect("描述欄")).collect();
    assert_eq!(exported, ["Echo", "Bravo", "alpha", "Delta", "Charlie"]);
}

#[tokio::test]
async fn expired_sort_is_consistent_with_rust_flag() {
    use chrono::{Duration, Local};

    let pool = test_pool().await;
    let today = Local::now().date_naive();
    let format_date = |date: chrono::NaiveDate| date.format("%Y-%m-%d").to_string();

    // 含時區邊界（今天、昨天＋年限 0）與閏日（2/29＋1 年）
    let cases = [
        ("今天到期", Some(today), Some(0)),
        ("昨天到期", Some(today - Duration::days(1)), Some(0)),
        (
            "閏日加一年",
            chrono::NaiveDate::from_ymd_opt(2020, 2, 29),
            Some(1),
        ),
        ("無年限", Some(today - Duration::days(4000)), None),
        ("無購置日期", None, Some(5)),
    ];
    for (index, (description, purchase_date, lifespan_years)) in cases.into_iter().enumerate() {
        create_asset(
            &pool,
            json!({
                "description": format!("{description} {index}"),
                "location": "機房",
                "purchase_date": purchase_date.map(format_date),
                "lifespan_years": lifespan_years,
            }),
        )
        .await;
    }

    for dir in ["asc", "desc"] {
        let (status, page) = send(
            &pool,
            Method::GET,
            &format!("/api/v1/assets?sort=expired&dir={dir}&per_page=10"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        let flags: Vec<bool> = page["items"]
            .as_array()
            .expect("items 為陣列")
            .iter()
            .map(|item| item["expired"].as_bool().expect("expired 為布林"))
            .collect();

        // 排序與 Rust 計算的旗標一致：升冪時 false 全在 true 之前
        let expected: Vec<bool> = if dir == "asc" {
            let mut sorted = flags.clone();
            sorted.sort_unstable();
            sorted
        } else {
            let mut sorted = flags.clone();
            sorted.sort_unstable_by(|a, b| b.cmp(a));
            sorted
        };
        assert_eq!(flags, expected, "dir={dir} 的屆齡排序與旗標不一致");
    }
}

#[tokio::test]
async fn invalid_sort_and_dir_return_400() {
    let pool = test_pool().await;

    let (status, body) = send(&pool, Method::GET, "/api/v1/assets?sort=id", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "validation_error");
    assert_eq!(body["details"]["field"], "sort");
    assert!(
        body["message"].as_str().expect("訊息為字串").contains("id"),
        "錯誤訊息應指出無效值"
    );

    let (status, body) = send(
        &pool,
        Method::GET,
        "/api/v1/assets?sort=description&dir=sideways",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "validation_error");
    assert_eq!(body["details"]["field"], "dir");

    // 有效值不受影響
    let (status, _) = send(
        &pool,
        Method::GET,
        "/api/v1/assets?sort=expired&dir=desc",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn tags_endpoint_deduplicates_case_insensitively_and_sorts() {
    let pool = test_pool().await;

    create_asset(
        &pool,
        json!({ "description": "一", "location": "機房", "tags": ["Zeta"] }),
    )
    .await;
    create_asset(
        &pool,
        json!({ "description": "二", "location": "機房", "tags": ["alpha", "Zeta"] }),
    )
    .await;
    create_asset(
        &pool,
        json!({ "description": "三", "location": "機房", "tags": ["ALPHA", "beta", "  "] }),
    )
    .await;
    create_asset(&pool, json!({ "description": "四", "location": "機房" })).await;

    let (status, body) = send(&pool, Method::GET, "/api/v1/tags", None).await;
    assert_eq!(status, StatusCode::OK);
    let tags: Vec<&str> = body["items"]
        .as_array()
        .expect("items 為陣列")
        .iter()
        .map(|value| value.as_str().expect("標籤為字串"))
        .collect();
    assert_eq!(
        tags,
        vec!["alpha", "beta", "Zeta"],
        "不分大小寫去重（保留最早原文）且依 NOCASE 排序"
    );
}

#[tokio::test]
async fn list_rows_include_all_assigned_ips() {
    let pool = test_pool().await;

    // 無指派：空陣列（前端顯示「—」）
    let bare = create_asset(
        &pool,
        json!({ "description": "無網路資產", "location": "機房" }),
    )
    .await;
    let bare_id = bare["id"].as_i64().expect("回應含 id");
    assert_eq!(
        assigned_ips_of(&pool, bare_id).await,
        Vec::<String>::new(),
        "無指派資產的 assigned_ips 為空陣列"
    );

    // 單筆指派
    let single = create_asset(
        &pool,
        json!({ "description": "單網段主機", "location": "機房" }),
    )
    .await;
    let single_id = single["id"].as_i64().expect("回應含 id");
    let single_interface = create_interface(&pool, single_id, json!({ "name": "eth0" })).await;
    let v4_a = create_subnet(&pool, json!({ "cidr": "10.0.0.0/29" })).await;
    assign_static(&pool, v4_a, "10.0.0.6", single_interface).await;
    assert_eq!(
        assigned_ips_of(&pool, single_id).await,
        vec!["10.0.0.6"],
        "單筆指派"
    );

    // 多介面、多網段、雙棧並存：v4 先、v6 後，同地址族依位址數值
    let multi = create_asset(
        &pool,
        json!({ "description": "多功能主機", "location": "機房" }),
    )
    .await;
    let multi_id = multi["id"].as_i64().expect("回應含 id");
    let first = create_interface(
        &pool,
        multi_id,
        json!({ "name": "eth0", "mac": "AA:BB:CC:DD:EE:00" }),
    )
    .await;
    let second = create_interface(
        &pool,
        multi_id,
        json!({ "name": "eth1", "mac": "AA:BB:CC:DD:EE:01" }),
    )
    .await;

    let v4_b = create_subnet(&pool, json!({ "cidr": "10.9.0.0/29" })).await;
    let v6_a = create_subnet(&pool, json!({ "cidr": "fd00::/64" })).await;
    let v6_b = create_subnet(&pool, json!({ "cidr": "fd00:1::/64" })).await;

    // 刻意與顯示順序不同的指派順序
    assign_static(&pool, v6_b, "fd00:1::5", first).await;
    assign_static(&pool, v4_a, "10.0.0.2", second).await;
    assign_static(&pool, v6_a, "fd00::a", second).await;
    assign_static(&pool, v4_b, "10.9.0.2", first).await;

    assert_eq!(
        assigned_ips_of(&pool, multi_id).await,
        vec!["10.0.0.2", "10.9.0.2", "fd00::a", "fd00:1::5"],
        "跨介面跨網段聚合；v4 先、v6 後，同族依位址數值"
    );

    // 指派後清單列仍維持原有資產欄位形狀；新增欄位僅存在於清單
    let (status, page) = send(&pool, Method::GET, "/api/v1/assets?per_page=200", None).await;
    assert_eq!(status, StatusCode::OK);
    let row = page["items"]
        .as_array()
        .expect("items 為陣列")
        .iter()
        .find(|item| item["id"].as_i64() == Some(multi_id))
        .expect("清單含該資產");
    assert_eq!(row["description"], "多功能主機");
    assert_eq!(row["location"], "機房");
    assert!(row["assigned_ips"].is_array(), "清單列含 assigned_ips");

    assert!(
        multi.get("assigned_ips").is_none(),
        "POST 回應不含 assigned_ips"
    );
    let (status, updated) = send(
        &pool,
        Method::PATCH,
        &format!("/api/v1/assets/{multi_id}"),
        Some(json!({ "note": "更新" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        updated.get("assigned_ips").is_none(),
        "PATCH 回應不含 assigned_ips"
    );
}

#[tokio::test]
async fn q_matches_mac_and_assigned_ips() {
    let pool = test_pool().await;

    let target = create_asset(
        &pool,
        json!({ "description": "有網路的伺服器", "location": "機房 A" }),
    )
    .await;
    let target_id = target["id"].as_i64().expect("回應含 id");
    let target_interface = create_interface(
        &pool,
        target_id,
        json!({ "name": "eth0", "mac": "AA:BB:CC:00:11:22" }),
    )
    .await;

    let other = create_asset(
        &pool,
        json!({ "description": "其他設備", "location": "機房 B" }),
    )
    .await;
    let other_id = other["id"].as_i64().expect("回應含 id");
    create_interface(
        &pool,
        other_id,
        json!({ "name": "eth0", "mac": "DE:AD:BE:EF:00:01" }),
    )
    .await;

    create_asset(
        &pool,
        json!({ "description": "純周邊", "location": "機房 A" }),
    )
    .await;

    let v4 = create_subnet(&pool, json!({ "cidr": "10.20.30.0/29" })).await;
    let v6 = create_subnet(&pool, json!({ "cidr": "fd00::/64" })).await;
    assign_static(&pool, v4, "10.20.30.4", target_interface).await;
    assign_static(&pool, v6, "fd00::abcd", target_interface).await;

    // MAC：子字串、大小寫無關
    for q in ["AA:BB:CC:00:11:22", "aa:bb", "00:11:22"] {
        assert_eq!(search_ids(&pool, q).await, vec![target_id], "q={q}");
    }
    assert_eq!(
        search_ids(&pool, "be:ef").await,
        vec![other_id],
        "另一介面的 MAC 片段"
    );

    // 已指派位址：IPv4／IPv6 子字串、大小寫無關、不因多介面重複列出
    for q in ["10.20.30.4", "20.30", "FD00::ABCD", "::abcd"] {
        assert_eq!(search_ids(&pool, q).await, vec![target_id], "q={q}");
    }

    // 與既有篩選可組合
    let (status, page) = send(
        &pool,
        Method::GET,
        &format!(
            "/api/v1/assets?q={}&location={}",
            encode("aa:bb"),
            encode("機房 A")
        ),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(page["total"], 1);
    assert_eq!(page["items"][0]["id"], target_id);

    let (_, page) = send(
        &pool,
        Method::GET,
        &format!(
            "/api/v1/assets?q={}&location={}",
            encode("aa:bb"),
            encode("機房 B")
        ),
        None,
    )
    .await;
    assert_eq!(page["total"], 0, "篩選仍為 AND 組合");

    // 無結果
    assert!(search_ids(&pool, "no-such-value").await.is_empty());
    assert!(search_ids(&pool, "10.20.30.99").await.is_empty());
}

#[tokio::test]
async fn export_assets_applies_filters_sort_and_ignores_pagination() {
    let pool = test_pool().await;

    // 空庫：僅標題列；標頭與檔名（含中文 filename* 編碼）
    let (status, headers, bytes) = export_bytes(&pool, "").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "靜態 export 路由優先於 /assets/{{id}}"
    );
    assert_eq!(headers[header::CONTENT_TYPE], "text/csv; charset=utf-8");
    let date = chrono::Local::now().format("%Y%m%d");
    let disposition = headers[header::CONTENT_DISPOSITION]
        .to_str()
        .expect("Content-Disposition 為文字");
    assert!(disposition.starts_with("attachment"), "{disposition}");
    assert!(
        disposition.contains(&format!(
            "filename*=UTF-8''%E8%B3%87%E7%94%A2%E5%8C%AF%E5%87%BA_{date}.csv"
        )),
        "中文檔名以 filename* 編碼：{disposition}"
    );
    let (headers_row, rows) = parse_export(&bytes);
    assert_eq!(
        headers_row,
        [
            "財產編號",
            "描述",
            "位置",
            "設備序號",
            "廠牌",
            "型號",
            "購置日期",
            "年限",
            "備註",
            "標籤",
            "MAC",
            "IPv4",
            "IPv6",
            "hostname"
        ],
        "14 欄與 ADR-0008 一致"
    );
    assert!(rows.is_empty(), "空庫僅有標題列");

    // A 含全部欄位與逗號備註；B 同位置；C 另一位置不同標籤；D 無標籤
    create_asset(
        &pool,
        json!({
            "property_no": "PC-001",
            "description": "Bravo 電腦",
            "location": "機房 A",
            "device_serial": "SN-A",
            "brand": "ASUS",
            "model": "BM6630",
            "purchase_date": "2024-01-15",
            "lifespan_years": 5,
            "note": "含,逗號",
            "tags": ["行政", "電腦"]
        }),
    )
    .await;
    create_asset(
        &pool,
        json!({ "description": "alpha 電腦", "location": "機房 A", "tags": ["行政"] }),
    )
    .await;
    create_asset(
        &pool,
        json!({ "description": "Charlie 印表機", "location": "機房 B", "tags": ["周邊"] }),
    )
    .await;
    create_asset(
        &pool,
        json!({ "description": "Delta 電腦", "location": "機房 B" }),
    )
    .await;

    // 無篩選：全部 4 筆；預設描述升冪（不分大小寫）
    let (_, _, bytes) = export_bytes(&pool, "").await;
    let (_, rows) = parse_export(&bytes);
    let descriptions: Vec<&str> = rows.iter().map(|row| row.get(1).expect("描述欄")).collect();
    assert_eq!(
        descriptions,
        ["alpha 電腦", "Bravo 電腦", "Charlie 印表機", "Delta 電腦"]
    );

    // 完整列：欄位、日期、標籤與引號往返
    let row = &rows[1];
    assert_eq!(row.get(0), Some("PC-001"));
    assert_eq!(row.get(3), Some("SN-A"));
    assert_eq!(row.get(4), Some("ASUS"));
    assert_eq!(row.get(5), Some("BM6630"));
    assert_eq!(row.get(6), Some("2024-01-15"), "日期為 YYYY-MM-DD");
    assert_eq!(row.get(7), Some("5"));
    assert_eq!(row.get(8), Some("含,逗號"), "含逗號欄位經引號往返");
    assert_eq!(row.get(9), Some("行政|電腦"), "標籤以 | 串接");

    // q 篩選
    let (_, _, bytes) = export_bytes(&pool, &format!("q={}", encode("電腦"))).await;
    let (_, rows) = parse_export(&bytes);
    assert_eq!(rows.len(), 3, "q=電腦 命中 3 筆");

    // location＋tag 組合篩選
    let (_, _, bytes) = export_bytes(
        &pool,
        &format!("location={}&tag={}", encode("機房 B"), encode("周邊")),
    )
    .await;
    let (_, rows) = parse_export(&bytes);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].get(1), Some("Charlie 印表機"));

    // 排序套用（描述降冪）
    let (_, _, bytes) = export_bytes(&pool, "sort=description&dir=desc").await;
    let (_, rows) = parse_export(&bytes);
    let descriptions: Vec<&str> = rows.iter().map(|row| row.get(1).expect("描述欄")).collect();
    assert_eq!(
        descriptions,
        ["Delta 電腦", "Charlie 印表機", "Bravo 電腦", "alpha 電腦"]
    );

    // 忽略分頁：per_page=1&page=2 仍匯出全部符合資產
    let (_, _, bytes) = export_bytes(&pool, "per_page=1&page=2").await;
    let (_, rows) = parse_export(&bytes);
    assert_eq!(rows.len(), 4, "匯出忽略分頁");
}

#[tokio::test]
async fn export_assets_rejects_invalid_sort_and_dir() {
    let pool = test_pool().await;

    let (status, _, bytes) = export_bytes(&pool, "sort=id").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let body: Value = serde_json::from_slice(&bytes).expect("錯誤回應為 JSON");
    assert_eq!(body["details"]["field"], "sort");

    let (status, _, bytes) = export_bytes(&pool, "sort=description&dir=sideways").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let body: Value = serde_json::from_slice(&bytes).expect("錯誤回應為 JSON");
    assert_eq!(body["details"]["field"], "dir");
}

#[tokio::test]
async fn export_assets_selects_interface_addresses_and_hostname() {
    let pool = test_pool().await;

    // 無介面：MAC／IPv4／IPv6／hostname 全空
    create_asset(
        &pool,
        json!({ "description": "純周邊", "location": "機房" }),
    )
    .await;

    // 多介面：第一介面無指派、第二介面有 → 取第二（含其 MAC）
    let multi = create_asset(
        &pool,
        json!({ "description": "多功能主機", "location": "機房" }),
    )
    .await;
    let multi_id = multi["id"].as_i64().expect("回應含 id");
    create_interface(&pool, multi_id, json!({ "name": "eth0" })).await;
    let second = create_interface(
        &pool,
        multi_id,
        json!({ "name": "eth1", "mac": "AA:BB:CC:DD:EE:11" }),
    )
    .await;

    let v4_low = create_subnet(&pool, json!({ "cidr": "10.0.0.0/29" })).await;
    let v4_high = create_subnet(&pool, json!({ "cidr": "10.0.0.8/29" })).await;
    let v6_low = create_subnet(&pool, json!({ "cidr": "fd00::/64" })).await;
    let v6_high = create_subnet(&pool, json!({ "cidr": "fd00:1::/64" })).await;

    // 刻意逆序指派；同介面同族取數值最小（v4 .2、v6 ::2）
    assign(&pool, v4_high, "10.0.0.10", second, "static", None).await;
    assign(&pool, v6_high, "fd00:1::a", second, "static", None).await;
    assign(
        &pool,
        v4_low,
        "10.0.0.2",
        second,
        "reservation",
        Some("pc-multi"),
    )
    .await;
    assign(&pool, v6_low, "fd00::2", second, "static", None).await;

    // hostname 非保留：最小 v4 為 static（另一筆保留的 hostname 不帶出）
    let static_asset = create_asset(
        &pool,
        json!({ "description": "非保留主機", "location": "機房" }),
    )
    .await;
    let static_id = static_asset["id"].as_i64().expect("回應含 id");
    let static_interface = create_interface(
        &pool,
        static_id,
        json!({ "name": "eth0", "mac": "AA:BB:CC:DD:EE:22" }),
    )
    .await;
    let other_low = create_subnet(&pool, json!({ "cidr": "10.9.0.0/29" })).await;
    let other_high = create_subnet(&pool, json!({ "cidr": "10.10.0.0/29" })).await;
    assign(
        &pool,
        other_low,
        "10.9.0.1",
        static_interface,
        "static",
        None,
    )
    .await;
    assign(
        &pool,
        other_high,
        "10.10.0.2",
        static_interface,
        "reservation",
        Some("unused"),
    )
    .await;

    let (status, _, bytes) = export_bytes(&pool, "").await;
    assert_eq!(status, StatusCode::OK);
    let (_, rows) = parse_export(&bytes);
    assert_eq!(rows.len(), 3);

    let by_description: std::collections::HashMap<&str, &csv::StringRecord> = rows
        .iter()
        .map(|row| (row.get(1).expect("描述欄"), row))
        .collect();

    let bare = by_description["純周邊"];
    assert_eq!(bare.get(10), Some(""), "無介面 MAC 空");
    assert_eq!(bare.get(11), Some(""), "無介面 IPv4 空");
    assert_eq!(bare.get(12), Some(""), "無介面 IPv6 空");
    assert_eq!(bare.get(13), Some(""), "無介面 hostname 空");

    let multi_row = by_description["多功能主機"];
    assert_eq!(
        multi_row.get(10),
        Some("aa:bb:cc:dd:ee:11"),
        "第一介面無指派、第二介面有 → 取第二介面（MAC 小寫冒號）"
    );
    assert_eq!(multi_row.get(11), Some("10.0.0.2"), "v4 取數值最小");
    assert_eq!(multi_row.get(12), Some("fd00::2"), "v6 取數值最小");
    assert_eq!(
        multi_row.get(13),
        Some("pc-multi"),
        "v4 為保留時帶 hostname"
    );

    let static_row = by_description["非保留主機"];
    assert_eq!(static_row.get(11), Some("10.9.0.1"), "v4 取數值最小");
    assert_eq!(
        static_row.get(13),
        Some(""),
        "最小 v4 為 static → hostname 空"
    );
}

#[tokio::test]
async fn asset_last_seen_takes_max_over_assignment_and_interface_mac_hits() {
    let pool = test_pool().await;
    let subnet = create_subnet(&pool, json!({ "cidr": "10.0.0.0/24" })).await;

    // alpha：介面 MAC aa:…:01、指派 10.0.0.5。
    let alpha = create_asset(&pool, json!({ "description": "alpha", "location": "機房" })).await;
    let alpha_id = alpha["id"].as_i64().expect("回應含 id");
    let alpha_interface = create_interface(
        &pool,
        alpha_id,
        json!({ "name": "eth0", "mac": "aa:bb:cc:dd:ee:01" }),
    )
    .await;
    assign_static(&pool, subnet, "10.0.0.5", alpha_interface).await;

    // 指派命中：宣告 MAC 與現況 MAC 不同仍算（比對位址、不比對 MAC）。
    insert_presence(
        &pool,
        subnet,
        "10.0.0.5",
        Some("2026-10-01T08:00:00Z"),
        Some("bb:bb:bb:bb:bb:bb"),
        Some("arp"),
        Some("2026-10-06T08:00:00Z"),
    )
    .await;
    // MAC 命中：未指派位址、時間較新 → 取最大值。
    insert_presence(
        &pool,
        subnet,
        "10.0.0.9",
        Some("2026-10-03T08:00:00Z"),
        Some("aa:bb:cc:dd:ee:01"),
        Some("kea_lease"),
        Some("2026-10-06T08:00:00Z"),
    )
    .await;
    // 無關現況：其他位址、其他 MAC 不影響。
    insert_presence(
        &pool,
        subnet,
        "10.0.0.7",
        Some("2026-10-05T08:00:00Z"),
        Some("cc:cc:cc:cc:cc:cc"),
        Some("arp"),
        Some("2026-10-06T08:00:00Z"),
    )
    .await;
    // 從未上線的指派位址：只有檢查時間，不計入。
    insert_presence(
        &pool,
        subnet,
        "10.0.0.6",
        None,
        Some("aa:bb:cc:dd:ee:01"),
        None,
        Some("2026-10-06T08:00:00Z"),
    )
    .await;

    assert_eq!(
        last_seen_of(&pool, alpha_id).await.as_deref(),
        Some("2026-10-03T08:00:00Z"),
        "指派命中與 MAC 命中取最大；只有檢查時間者不計入"
    );
    assert_eq!(
        detail_last_seen_of(&pool, alpha_id).await.as_deref(),
        Some("2026-10-03T08:00:00Z"),
        "詳情回應含相同的 last_seen_at"
    );
}

#[tokio::test]
async fn asset_last_seen_matches_mac_case_insensitively_and_per_asset() {
    let pool = test_pool().await;
    let subnet = create_subnet(&pool, json!({ "cidr": "10.0.0.0/24" })).await;

    let alpha = create_asset(&pool, json!({ "description": "alpha", "location": "機房" })).await;
    let alpha_id = alpha["id"].as_i64().expect("回應含 id");
    create_interface(
        &pool,
        alpha_id,
        json!({ "name": "eth0", "mac": "aa:bb:cc:dd:ee:01" }),
    )
    .await;

    let bravo = create_asset(&pool, json!({ "description": "Bravo", "location": "機房" })).await;
    let bravo_id = bravo["id"].as_i64().expect("回應含 id");
    create_interface(
        &pool,
        bravo_id,
        json!({ "name": "eth0", "mac": "aa:bb:cc:dd:ee:02" }),
    )
    .await;

    // charlie：無介面、無現況 → null。
    let charlie = create_asset(
        &pool,
        json!({ "description": "Charlie", "location": "機房" }),
    )
    .await;
    let charlie_id = charlie["id"].as_i64().expect("回應含 id");

    // alpha 的現況以大寫寫入（模擬既有資料）：比對仍不分大小寫。
    insert_presence(
        &pool,
        subnet,
        "10.0.0.9",
        Some("2026-10-02T08:00:00Z"),
        Some("AA:BB:CC:DD:EE:01"),
        Some("arp"),
        Some("2026-10-06T08:00:00Z"),
    )
    .await;
    insert_presence(
        &pool,
        subnet,
        "10.0.0.8",
        Some("2026-10-04T08:00:00Z"),
        Some("aa:bb:cc:dd:ee:02"),
        Some("arp"),
        Some("2026-10-06T08:00:00Z"),
    )
    .await;

    assert_eq!(
        last_seen_of(&pool, alpha_id).await.as_deref(),
        Some("2026-10-02T08:00:00Z"),
        "大寫現況 MAC 仍命中 alpha 介面"
    );
    assert_eq!(
        last_seen_of(&pool, bravo_id).await.as_deref(),
        Some("2026-10-04T08:00:00Z"),
        "各資產只取自己命中的現況"
    );
    assert_eq!(
        last_seen_of(&pool, charlie_id).await,
        None,
        "無命中現況為 null"
    );
    assert_eq!(detail_last_seen_of(&pool, charlie_id).await, None);
}

#[tokio::test]
async fn list_sorts_by_last_seen_with_nulls_last() {
    let pool = test_pool().await;
    let subnet = create_subnet(&pool, json!({ "cidr": "10.0.0.0/24" })).await;

    // alpha：2026-10-01；Bravo：2026-10-03；Charlie：無現況（NULL 固定最後）。
    let alpha = create_asset(&pool, json!({ "description": "alpha", "location": "機房" })).await;
    let alpha_id = alpha["id"].as_i64().expect("回應含 id");
    create_interface(
        &pool,
        alpha_id,
        json!({ "name": "eth0", "mac": "aa:bb:cc:dd:ee:01" }),
    )
    .await;
    let bravo = create_asset(&pool, json!({ "description": "Bravo", "location": "機房" })).await;
    let bravo_id = bravo["id"].as_i64().expect("回應含 id");
    create_interface(
        &pool,
        bravo_id,
        json!({ "name": "eth0", "mac": "aa:bb:cc:dd:ee:02" }),
    )
    .await;
    create_asset(
        &pool,
        json!({ "description": "Charlie", "location": "機房" }),
    )
    .await;

    insert_presence(
        &pool,
        subnet,
        "10.0.0.9",
        Some("2026-10-01T08:00:00Z"),
        Some("aa:bb:cc:dd:ee:01"),
        Some("arp"),
        Some("2026-10-06T08:00:00Z"),
    )
    .await;
    insert_presence(
        &pool,
        subnet,
        "10.0.0.8",
        Some("2026-10-03T08:00:00Z"),
        Some("aa:bb:cc:dd:ee:02"),
        Some("arp"),
        Some("2026-10-06T08:00:00Z"),
    )
    .await;

    assert_eq!(
        descriptions(&pool, "sort=last_seen&dir=asc&per_page=10").await,
        vec!["alpha", "Bravo", "Charlie"],
        "升冪：時間舊到新，NULL 最後"
    );
    assert_eq!(
        descriptions(&pool, "sort=last_seen&dir=desc&per_page=10").await,
        vec!["Bravo", "alpha", "Charlie"],
        "降冪：時間新到舊，NULL 仍最後"
    );

    // 分頁以排序後序列切頁，總數不變。
    let (status, page) = send(
        &pool,
        Method::GET,
        "/api/v1/assets?sort=last_seen&dir=asc&per_page=2&page=2",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(page["total"], 3);
    let page_descriptions: Vec<&str> = page["items"]
        .as_array()
        .expect("items 為陣列")
        .iter()
        .map(|item| item["description"].as_str().expect("描述為字串"))
        .collect();
    assert_eq!(page_descriptions, ["Charlie"]);

    // 匯出沿用同一排序。
    let (status, _, bytes) = export_bytes(&pool, "sort=last_seen&dir=desc").await;
    assert_eq!(status, StatusCode::OK);
    let (_, rows) = parse_export(&bytes);
    let exported: Vec<&str> = rows.iter().map(|row| row.get(1).expect("描述欄")).collect();
    assert_eq!(exported, ["Bravo", "alpha", "Charlie"]);

    // 白名單外仍 400（新增欄位不放寬既有驗證）。
    let (status, body) = send(&pool, Method::GET, "/api/v1/assets?sort=id", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["details"]["field"], "sort");
}

#[tokio::test]
async fn list_rows_include_lending_brief() {
    let pool = test_pool().await;

    // 未出借：lending 為 null
    let bare = create_asset(
        &pool,
        json!({ "description": "未出借資產", "location": "機房" }),
    )
    .await;
    let bare_id = bare["id"].as_i64().expect("回應含 id");
    assert_eq!(
        lending_of(&pool, bare_id).await,
        None,
        "未出借資產 lending 為 null"
    );

    // 出借中：帶借出摘要（id／借用人／借出時間／預計歸還日）
    let open = create_asset(
        &pool,
        json!({ "property_no": "PC-900", "description": "出借中資產", "location": "機房" }),
    )
    .await;
    let open_id = open["id"].as_i64().expect("回應含 id");
    let lending_id = create_lending(
        &pool,
        open_id,
        json!({ "borrower": "王小明", "due_at": "2030-01-01" }),
    )
    .await;
    let brief = lending_of(&pool, open_id)
        .await
        .expect("出借中資產含 lending");
    assert_eq!(brief["id"], lending_id);
    assert_eq!(brief["borrower"], "王小明");
    assert_eq!(brief["due_at"], "2030-01-01");
    assert!(brief["lent_at"].is_string(), "lent_at 非空");
    assert!(brief.get("returned_at").is_none(), "摘要不含歸還時間");
    assert_eq!(lending_of(&pool, bare_id).await, None, "未出借資產不受影響");

    // 已歸還：lending 回 null（摘要只反映未歸還紀錄）
    return_lending(&pool, lending_id).await;
    assert_eq!(
        lending_of(&pool, open_id).await,
        None,
        "歸還後 lending 回 null"
    );
}
