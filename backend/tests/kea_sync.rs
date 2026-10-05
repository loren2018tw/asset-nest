//! Kea 推送與完整同步整合測試：以本機 stub Kea 驗證命令、差異與失敗處理
//!（見 `docs/adr/0011`、`docs/adr/0013`）。
//!
//! stub 只實作本系統使用的命令子集（version-get／config-get／reservation-*／
//! subnet4-update／config-write），資料存記憶體；不觸及真機。

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::extract::State;
use axum::http::{Method, Request, StatusCode, header};
use axum::routing::post;
use axum::{Json, Router};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use sqlx::SqlitePool;
use sqlx::sqlite::SqlitePoolOptions;
use tower::ServiceExt;

use asset_nest::kea::http::Client;
use asset_nest::{AppState, app};

// ---------- stub Kea ----------

/// stub 中的一筆保留。
#[derive(Debug, Clone, PartialEq, Eq)]
struct StubReservation {
    ip_address: String,
    hw_address: String,
    hostname: Option<String>,
}

/// 記憶體版假 Kea；`clone` 共用同一份狀態。
#[derive(Clone)]
struct StubKea {
    hosts: Arc<Mutex<HashMap<i64, Vec<StubReservation>>>>,
    fail: Arc<Mutex<Vec<String>>>,
    config_writes: Arc<Mutex<usize>>,
    log: Arc<Mutex<Vec<String>>>,
    /// 完整 `subnet4` 物件（subnet-id → 設定；`config-get` 用）。
    subnets: Arc<Mutex<BTreeMap<i64, Value>>>,
}

impl Default for StubKea {
    /// 預設一個受管網段（subnet-id 1、`10.0.0.0/24`、無 pool／option-data），
    /// 與既有保留同步測試的資料相符。
    fn default() -> Self {
        Self {
            hosts: Arc::default(),
            fail: Arc::default(),
            config_writes: Arc::default(),
            log: Arc::default(),
            subnets: Arc::new(Mutex::new(BTreeMap::from([(
                1,
                json!({ "id": 1, "subnet": "10.0.0.0/24" }),
            )]))),
        }
    }
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

    fn log(&self) -> Vec<String> {
        self.log.lock().expect("log").clone()
    }

    fn config_writes(&self) -> usize {
        *self.config_writes.lock().expect("config_writes")
    }

    fn fail_commands(&self, commands: &[&str]) {
        *self.fail.lock().expect("fail") = commands.iter().map(|c| c.to_string()).collect();
    }

    fn set_host(&self, subnet_id: i64, ip: &str, hw: &str, hostname: Option<&str>) {
        let mut hosts = self.hosts.lock().expect("hosts");
        let list = hosts.entry(subnet_id).or_default();
        list.retain(|host| host.ip_address != ip);
        list.push(StubReservation {
            ip_address: ip.to_string(),
            hw_address: hw.to_string(),
            hostname: hostname.map(str::to_string),
        });
    }

    fn clear_hosts(&self) {
        self.hosts.lock().expect("hosts").clear();
    }

    fn hosts(&self, subnet_id: i64) -> Vec<StubReservation> {
        self.hosts
            .lock()
            .expect("hosts")
            .get(&subnet_id)
            .cloned()
            .unwrap_or_default()
    }

    /// 覆寫整個 `subnet4` 條目。
    fn set_subnet(&self, id: i64, subnet: Value) {
        self.subnets.lock().expect("subnets").insert(id, subnet);
    }

    fn subnet(&self, id: i64) -> Value {
        self.subnets
            .lock()
            .expect("subnets")
            .get(&id)
            .cloned()
            .unwrap_or(Value::Null)
    }
}

/// stub 的命令處理：回應格式比照 Kea（單元素陣列、result 0／1／3）。
async fn handle(State(stub): State<StubKea>, Json(payload): Json<Value>) -> Json<Value> {
    let command = payload
        .get("command")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    stub.log.lock().expect("log").push(command.clone());

    if stub
        .fail
        .lock()
        .expect("fail")
        .iter()
        .any(|failed| failed == &command)
    {
        return Json(json!([{ "result": 1, "text": "simulated failure" }]));
    }

    let arguments = payload.get("arguments").cloned().unwrap_or(Value::Null);
    let response = match command.as_str() {
        "version-get" => json!([{
            "result": 0,
            "text": "Kea",
            "arguments": { "version": "3.2.1" }
        }]),
        "config-get" => {
            let subnets: Vec<Value> = stub
                .subnets
                .lock()
                .expect("subnets")
                .values()
                .cloned()
                .collect();
            json!([{
                "result": 0,
                "arguments": { "Dhcp4": { "subnet4": subnets } }
            }])
        }
        "reservation-get-all" => {
            let subnet_id = arguments
                .get("subnet-id")
                .and_then(Value::as_i64)
                .unwrap_or(0);
            let hosts = stub.hosts(subnet_id);
            if hosts.is_empty() {
                json!([{ "result": 3, "text": "0 IPv4 host(s) found." }])
            } else {
                let entries: Vec<Value> = hosts
                    .iter()
                    .map(|host| {
                        let mut entry = json!({
                            "ip-address": host.ip_address,
                            "hw-address": host.hw_address,
                        });
                        if let Some(hostname) = &host.hostname {
                            entry["hostname"] = json!(hostname);
                        }
                        entry
                    })
                    .collect();
                json!([{
                    "result": 0,
                    "text": format!("{} IPv4 host(s) found.", entries.len()),
                    "arguments": { "hosts": entries }
                }])
            }
        }
        "reservation-add" => {
            // 比照真機：未顯式指定 memory 目標時，主機資料庫不存在 → 失敗。
            if arguments.get("operation-target").and_then(Value::as_str) != Some("memory") {
                return Json(json!([{
                    "result": 1,
                    "text": "Host database not available, cannot add host."
                }]));
            }
            let reservation = &arguments["reservation"];
            let subnet_id = reservation["subnet-id"].as_i64().unwrap_or(0);
            let mut hosts = stub.hosts.lock().expect("hosts");
            let list = hosts.entry(subnet_id).or_default();
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
            list.retain(|host| host.ip_address != entry.ip_address);
            list.push(entry);
            json!([{ "result": 0, "text": "Host added." }])
        }
        "reservation-del" => {
            // 比照真機：未顯式指定 memory 目標時，主機資料庫不存在 → 失敗。
            if arguments.get("operation-target").and_then(Value::as_str) != Some("memory") {
                return Json(json!([{
                    "result": 1,
                    "text": "Host database not available, cannot delete host."
                }]));
            }
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
        "subnet4-update" => {
            let Some(subnet) = arguments
                .get("subnet4")
                .and_then(Value::as_array)
                .and_then(|entries| entries.first())
                .cloned()
            else {
                return Json(json!([{ "result": 1, "text": "missing subnet4" }]));
            };

            // 比照真機：更新不可帶主機保留（見 .scratch/kea-subnet-sync/issues/04）。
            if subnet.get("reservations").is_some() {
                return Json(json!([{
                    "result": 1,
                    "text": "must not specify host reservations with 'subnet4-update'."
                }]));
            }

            let id = subnet["id"].as_i64().unwrap_or(0);
            // 比照真機：保留存於 CfgHosts，網段取代後 `config-get` 仍會合併呈現。
            let mut stored = subnet;
            let previous = stub.subnet(id);
            if let Some(reservations) = previous.get("reservations") {
                stored["reservations"] = reservations.clone();
            }
            stub.set_subnet(id, stored);
            json!([{ "result": 0, "text": "IPv4 subnet updated" }])
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

/// 建立測試資料庫並套用 migrations。
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

fn dist_dir() -> std::path::PathBuf {
    std::env::temp_dir().join("asset-nest-test-no-dist")
}

/// 建立附掛 stub Kea 的 AppState。
async fn test_state() -> (AppState, StubKea) {
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

async fn create_subnet(state: &AppState, cidr: &str, kea_subnet_id: Option<i64>) -> i64 {
    let mut body = json!({ "cidr": cidr });
    if let Some(kea_subnet_id) = kea_subnet_id {
        body["kea_subnet_id"] = json!(kea_subnet_id);
    }

    let (status, json) = send(state, Method::POST, "/api/v1/subnets", Some(body)).await;
    assert_eq!(status, StatusCode::CREATED, "新增網段應成功：{json}");
    json["id"].as_i64().expect("回應含 id")
}

/// 建立含 gateway 與 pool 的受管網段。
async fn create_subnet_full(
    state: &AppState,
    cidr: &str,
    kea_subnet_id: i64,
    gateway: Option<&str>,
    pools: &[(&str, &str)],
) -> i64 {
    let pools: Vec<Value> = pools
        .iter()
        .map(|(start, end)| json!({ "start_ip": start, "end_ip": end }))
        .collect();
    let mut body = json!({ "cidr": cidr, "kea_subnet_id": kea_subnet_id, "pools": pools });
    if let Some(gateway) = gateway {
        body["gateway"] = json!(gateway);
    }

    let (status, json) = send(state, Method::POST, "/api/v1/subnets", Some(body)).await;
    assert_eq!(status, StatusCode::CREATED, "新增網段應成功：{json}");
    json["id"].as_i64().expect("回應含 id")
}

/// 網段層同步情境：asset-nest 端建立帶 gateway／pool 的網段；Kea 端 subnet 1
/// 有 `.30–.40`（帶 `client-classes` 屬性）＋`.50–.60`、routers `.254`、
/// `reservations`（比照真機 config-get 合併主機保留）、`domain-name-servers`
/// option 與 `interface` 欄位（驗證原樣保留）。
async fn settings_fixture(
    state: &AppState,
    stub: &StubKea,
    gateway: Option<&str>,
    pools: &[(&str, &str)],
) -> i64 {
    let subnet = create_subnet_full(state, "10.0.0.0/24", 1, gateway, pools).await;
    stub.set_subnet(
        1,
        json!({
            "id": 1,
            "subnet": "10.0.0.0/24",
            "interface": "eth0",
            "pools": [
                { "pool": "10.0.0.30 - 10.0.0.40", "client-classes": ["keep-me"] },
                { "pool": "10.0.0.50 - 10.0.0.60" }
            ],
            "reservations": [
                { "hw-address": "aa:bb:cc:dd:ee:ff", "ip-address": "10.0.0.99" }
            ],
            "option-data": [
                { "name": "domain-name-servers", "code": 6, "space": "dhcp4", "data": "10.0.0.53" },
                { "name": "routers", "code": 3, "space": "dhcp4", "csv-format": true, "data": "10.0.0.254" }
            ]
        }),
    );
    subnet
}

async fn create_asset(state: &AppState, description: &str) -> i64 {
    let (status, json) = send(
        state,
        Method::POST,
        "/api/v1/assets",
        Some(json!({ "description": description, "location": "機房 A" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "新增資產應成功：{json}");
    json["id"].as_i64().expect("回應含 id")
}

async fn create_interface(state: &AppState, asset_id: i64, mac: &str) -> i64 {
    let (status, json) = send(
        state,
        Method::POST,
        &format!("/api/v1/assets/{asset_id}/interfaces"),
        Some(json!({ "mac": mac })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "新增介面應成功：{json}");
    json["id"].as_i64().expect("回應含 id")
}

async fn put_assignment(
    state: &AppState,
    subnet_id: i64,
    address: &str,
    interface_id: i64,
    purpose: &str,
    hostname: Option<&str>,
) -> (StatusCode, Value) {
    send(
        state,
        Method::PUT,
        &format!("/api/v1/subnets/{subnet_id}/ips/{address}/assignment"),
        Some(json!({
            "interface_id": interface_id,
            "purpose": purpose,
            "hostname": hostname,
        })),
    )
    .await
}

async fn delete_assignment(state: &AppState, subnet_id: i64, address: &str) -> (StatusCode, Value) {
    send(
        state,
        Method::DELETE,
        &format!("/api/v1/subnets/{subnet_id}/ips/{address}/assignment"),
        None,
    )
    .await
}

/// 完整同步情境：DB 有 .5（新增）／.6（更新）／.7（不變）／.9＋.10（衝突跳過），
/// Kea 端有 .6（舊 hostname）／.7（相同）／.8（多餘）。
async fn sync_fixture(state: &AppState, stub: &StubKea) -> i64 {
    let subnet = create_subnet(state, "10.0.0.0/24", Some(1)).await;
    let asset = create_asset(state, "同步主機").await;

    let i05 = create_interface(state, asset, "aa:aa:aa:aa:aa:05").await;
    let i06 = create_interface(state, asset, "aa:aa:aa:aa:aa:06").await;
    let i07 = create_interface(state, asset, "aa:aa:aa:aa:aa:07").await;
    let i09a = create_interface(state, asset, "aa:aa:aa:aa:aa:09").await;
    let i09b = create_interface(state, asset, "aa:aa:aa:aa:aa:09").await;

    for (address, interface_id, hostname) in [
        ("10.0.0.5", i05, Some("pc-5")),
        ("10.0.0.6", i06, Some("pc-6")),
        ("10.0.0.7", i07, Some("pc-7")),
        ("10.0.0.9", i09a, None),
        ("10.0.0.10", i09b, None),
    ] {
        let (status, body) = put_assignment(
            state,
            subnet,
            address,
            interface_id,
            "reservation",
            hostname,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{address} 指派應成功：{body}");
    }

    stub.clear_hosts();
    stub.set_host(1, "10.0.0.6", "aa:aa:aa:aa:aa:06", None);
    stub.set_host(1, "10.0.0.7", "aa:aa:aa:aa:aa:07", Some("pc-7"));
    stub.set_host(1, "10.0.0.8", "aa:aa:aa:aa:aa:08", None);

    subnet
}

// ---------- 單筆推送 ----------

#[tokio::test]
async fn assign_reservation_pushes_upsert_and_writes_config() {
    let (state, stub) = test_state().await;
    let subnet = create_subnet(&state, "10.0.0.0/24", Some(1)).await;
    let asset = create_asset(&state, "主機").await;
    let interface = create_interface(&state, asset, "aa:aa:aa:aa:aa:01").await;

    let (status, body) = put_assignment(
        &state,
        subnet,
        "10.0.0.5",
        interface,
        "reservation",
        Some("pc-5"),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "指派應成功：{body}");
    assert_eq!(body["kea_sync"]["status"], "ok");
    assert_eq!(
        stub.hosts(1),
        vec![StubReservation {
            ip_address: "10.0.0.5".to_string(),
            hw_address: "aa:aa:aa:aa:aa:01".to_string(),
            hostname: Some("pc-5".to_string()),
        }]
    );

    let log = stub.log();
    assert!(log.contains(&"reservation-del".to_string()), "upsert 先刪");
    assert!(log.contains(&"reservation-add".to_string()), "upsert 後增");
    assert_eq!(stub.config_writes(), 1, "推送後寫入 Kea 設定檔");
}

#[tokio::test]
async fn static_or_unmanaged_subnet_does_not_touch_kea() {
    let (state, stub) = test_state().await;
    let managed = create_subnet(&state, "10.0.0.0/24", Some(1)).await;
    let unmanaged = create_subnet(&state, "10.1.0.0/24", None).await;
    let asset = create_asset(&state, "主機").await;
    let first = create_interface(&state, asset, "aa:aa:aa:aa:aa:01").await;
    let second = create_interface(&state, asset, "aa:aa:aa:aa:aa:02").await;

    let (status, body) = put_assignment(&state, managed, "10.0.0.5", first, "static", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.get("kea_sync").is_none(), "static 不涉同步");

    let (status, body) =
        put_assignment(&state, unmanaged, "10.1.0.5", second, "reservation", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.get("kea_sync").is_none(), "未受管網段不涉同步");

    assert!(stub.log().is_empty(), "不應送出任何 Kea 命令");
}

#[tokio::test]
async fn reservation_to_static_removes_kea_reservation() {
    let (state, stub) = test_state().await;
    let subnet = create_subnet(&state, "10.0.0.0/24", Some(1)).await;
    let asset = create_asset(&state, "主機").await;
    let interface = create_interface(&state, asset, "aa:aa:aa:aa:aa:01").await;

    let (status, _) = put_assignment(
        &state,
        subnet,
        "10.0.0.5",
        interface,
        "reservation",
        Some("pc-5"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(stub.hosts(1).len(), 1);

    let (status, body) =
        put_assignment(&state, subnet, "10.0.0.5", interface, "static", None).await;

    assert_eq!(status, StatusCode::OK, "改用途應成功：{body}");
    assert_eq!(body["kea_sync"]["status"], "ok");
    assert!(stub.hosts(1).is_empty(), "改為 static 後刪除 Kea 保留");
    assert_eq!(stub.config_writes(), 2);
}

#[tokio::test]
async fn cancel_removes_kea_reservation_and_reports_sync() {
    let (state, stub) = test_state().await;
    let subnet = create_subnet(&state, "10.0.0.0/24", Some(1)).await;
    let asset = create_asset(&state, "主機").await;
    let interface = create_interface(&state, asset, "aa:aa:aa:aa:aa:01").await;

    let (status, _) = put_assignment(
        &state,
        subnet,
        "10.0.0.5",
        interface,
        "reservation",
        Some("pc-5"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = delete_assignment(&state, subnet, "10.0.0.5").await;

    assert_eq!(status, StatusCode::OK, "取消指派應成功：{body}");
    assert_eq!(body["kea_sync"]["status"], "ok");
    assert!(stub.hosts(1).is_empty(), "取消後刪除 Kea 保留");
    assert_eq!(stub.config_writes(), 2);
}

#[tokio::test]
async fn kea_failure_keeps_assignment_and_reports_warning() {
    let (state, stub) = test_state().await;
    stub.fail_commands(&["reservation-del", "reservation-add"]);
    let subnet = create_subnet(&state, "10.0.0.0/24", Some(1)).await;
    let asset = create_asset(&state, "主機").await;
    let interface = create_interface(&state, asset, "aa:aa:aa:aa:aa:01").await;

    let (status, body) = put_assignment(
        &state,
        subnet,
        "10.0.0.5",
        interface,
        "reservation",
        Some("pc-5"),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "本地仍應成功：{body}");
    assert_eq!(body["kea_sync"]["status"], "failed");
    assert!(
        body["kea_sync"]["message"]
            .as_str()
            .is_some_and(|message| !message.is_empty()),
        "應帶失敗訊息"
    );
    assert_eq!(stub.config_writes(), 0, "失敗時不寫設定檔");

    let (_, page) = send(
        &state,
        Method::GET,
        &format!("/api/v1/subnets/{subnet}/ips?status=reservation"),
        None,
    )
    .await;
    assert_eq!(page["total"], 1, "指派已保存，待完整同步修復");
}

// ---------- 完整同步 ----------

#[tokio::test]
async fn plan_reports_add_update_delete_and_skips_conflicts() {
    let (state, stub) = test_state().await;
    sync_fixture(&state, &stub).await;

    let (status, plan) = send(&state, Method::GET, "/api/v1/kea/sync/plan", None).await;

    assert_eq!(status, StatusCode::OK, "計畫應成功：{plan}");
    assert_eq!(plan["totals"]["add"], 1);
    assert_eq!(plan["totals"]["update"], 1);
    assert_eq!(plan["totals"]["delete"], 1);
    assert_eq!(plan["totals"]["skipped"], 2);

    let subnet = &plan["subnets"][0];
    assert_eq!(subnet["kea_subnet_id"], 1);
    assert_eq!(subnet["add"][0]["ip_address"], "10.0.0.5");
    assert_eq!(
        subnet["add"][0]["desired"]["hw_address"],
        "aa:aa:aa:aa:aa:05"
    );
    assert_eq!(subnet["update"][0]["ip_address"], "10.0.0.6");
    assert_eq!(subnet["update"][0]["desired"]["hostname"], "pc-6");
    assert!(subnet["update"][0]["current"]["hostname"].is_null());
    assert_eq!(subnet["delete"][0]["ip_address"], "10.0.0.8");

    let skipped: Vec<&str> = subnet["skipped"]
        .as_array()
        .expect("skipped 為陣列")
        .iter()
        .filter_map(|skip| skip["ip_address"].as_str())
        .collect();
    assert!(
        skipped.contains(&"10.0.0.9") && skipped.contains(&"10.0.0.10"),
        "衝突跳過"
    );
}

#[tokio::test]
async fn apply_executes_plan_and_writes_config_once() {
    let (state, stub) = test_state().await;
    sync_fixture(&state, &stub).await;
    let writes_before = stub.config_writes();

    let (status, report) = send(&state, Method::POST, "/api/v1/kea/sync", None).await;

    assert_eq!(status, StatusCode::OK, "套用應成功：{report}");
    let subnet = &report["subnets"][0];
    assert_eq!(subnet["added"], 1);
    assert_eq!(subnet["updated"], 1);
    assert_eq!(subnet["deleted"], 1);
    assert_eq!(
        subnet["failures"]
            .as_array()
            .expect("failures 為陣列")
            .len(),
        0
    );
    assert_eq!(report["config_write"], "ok");
    assert_eq!(
        stub.config_writes(),
        writes_before + 1,
        "整批只寫一次設定檔"
    );

    let hosts = stub.hosts(1);
    let by_ip = |ip: &str| hosts.iter().find(|host| host.ip_address == ip);
    assert_eq!(
        by_ip("10.0.0.5").expect("新增的保留").hw_address,
        "aa:aa:aa:aa:aa:05"
    );
    assert_eq!(
        by_ip("10.0.0.6").expect("更新的保留").hostname.as_deref(),
        Some("pc-6"),
        "hostname 已更新"
    );
    assert!(by_ip("10.0.0.8").is_none(), "多餘保留已刪除");
    assert!(
        by_ip("10.0.0.9").is_none() && by_ip("10.0.0.10").is_none(),
        "衝突項不推送"
    );
}

#[tokio::test]
async fn plan_reports_pool_and_gateway_changes() {
    let (state, stub) = test_state().await;
    settings_fixture(
        &state,
        &stub,
        Some("10.0.0.1"),
        &[("10.0.0.10", "10.0.0.20"), ("10.0.0.30", "10.0.0.40")],
    )
    .await;

    let (status, plan) = send(&state, Method::GET, "/api/v1/kea/sync/plan", None).await;

    assert_eq!(status, StatusCode::OK, "計畫應成功：{plan}");
    assert_eq!(plan["totals"]["add"], 0, "無保留變更");
    assert_eq!(plan["totals"]["pool_add"], 1);
    assert_eq!(plan["totals"]["pool_delete"], 1);
    assert_eq!(plan["totals"]["gateway"], 1);

    let subnet = &plan["subnets"][0];
    assert_eq!(subnet["pool_add"][0], "10.0.0.10-10.0.0.20");
    assert_eq!(subnet["pool_delete"][0], "10.0.0.50-10.0.0.60");
    assert_eq!(subnet["gateway"]["current"], "10.0.0.254");
    assert_eq!(subnet["gateway"]["desired"], "10.0.0.1");
}

#[tokio::test]
async fn apply_rebuilds_pools_and_gateway_keeping_other_fields() {
    let (state, stub) = test_state().await;
    settings_fixture(
        &state,
        &stub,
        Some("10.0.0.1"),
        &[("10.0.0.10", "10.0.0.20"), ("10.0.0.30", "10.0.0.40")],
    )
    .await;

    let (status, report) = send(&state, Method::POST, "/api/v1/kea/sync", None).await;

    assert_eq!(status, StatusCode::OK, "套用應成功：{report}");
    let subnet = &report["subnets"][0];
    assert_eq!(subnet["pool_added"], 1);
    assert_eq!(subnet["pool_deleted"], 1);
    assert_eq!(subnet["gateway_updated"], true);
    assert!(subnet.get("settings_error").is_none(), "成功時省略錯誤");
    assert_eq!(report["config_write"], "ok");
    assert_eq!(
        stub.log()
            .iter()
            .filter(|command| command.as_str() == "subnet4-update")
            .count(),
        1,
        "單一網段更新"
    );
    assert_eq!(stub.config_writes(), 1);

    let updated = stub.subnet(1);
    assert_eq!(updated["interface"], "eth0", "其他欄位原樣保留");
    let pools = updated["pools"].as_array().expect("pools 陣列");
    let by_range = |range: &str| {
        pools.iter().find(|entry| {
            entry["pool"]
                .as_str()
                .is_some_and(|value| value.replace(' ', "") == range)
        })
    };
    assert_eq!(
        by_range("10.0.0.10-10.0.0.20").expect("新增的 pool")["pool"],
        "10.0.0.10 - 10.0.0.20"
    );
    assert_eq!(
        by_range("10.0.0.30-10.0.0.40").expect("保留的 pool")["client-classes"][0],
        "keep-me",
        "同範圍的既有條目原樣保留"
    );
    assert!(by_range("10.0.0.50-10.0.0.60").is_none(), "多餘 pool 刪除");

    let options = updated["option-data"].as_array().expect("option-data");
    let routers = options
        .iter()
        .find(|option| option["name"] == "routers")
        .expect("routers 存在");
    assert_eq!(routers["data"], "10.0.0.1");
    assert!(
        options
            .iter()
            .any(|option| option["name"] == "domain-name-servers"),
        "其他 option 保留"
    );
    assert_eq!(
        updated["reservations"][0]["ip-address"], "10.0.0.99",
        "主機保留不受網段更新影響（送出前剝除 reservations）"
    );
}

#[tokio::test]
async fn apply_removes_routers_when_gateway_unset() {
    let (state, stub) = test_state().await;
    settings_fixture(&state, &stub, None, &[("10.0.0.30", "10.0.0.40")]).await;

    let (status, report) = send(&state, Method::POST, "/api/v1/kea/sync", None).await;

    assert_eq!(status, StatusCode::OK, "套用應成功：{report}");
    assert_eq!(report["subnets"][0]["gateway_updated"], true);

    let updated = stub.subnet(1);
    let options = updated["option-data"].as_array().expect("option-data");
    assert!(
        !options.iter().any(|option| option["name"] == "routers"),
        "gateway 未設＝移除 routers"
    );
    assert!(
        options
            .iter()
            .any(|option| option["name"] == "domain-name-servers"),
        "其他 option 保留"
    );
}

#[tokio::test]
async fn matching_subnet_settings_are_not_pushed() {
    let (state, stub) = test_state().await;
    settings_fixture(
        &state,
        &stub,
        Some("10.0.0.254"),
        &[("10.0.0.30", "10.0.0.40")],
    )
    .await;
    // Kea 端只留同範圍的 pool 與相同 routers。
    stub.set_subnet(
        1,
        json!({
            "id": 1,
            "subnet": "10.0.0.0/24",
            "pools": [ { "pool": "10.0.0.30 - 10.0.0.40" } ],
            "option-data": [ { "name": "routers", "code": 3, "space": "dhcp4", "data": "10.0.0.254" } ]
        }),
    );
    let writes_before = stub.config_writes();

    let (status, report) = send(&state, Method::POST, "/api/v1/kea/sync", None).await;

    assert_eq!(status, StatusCode::OK, "套用應成功：{report}");
    assert_eq!(report["config_write"], "skipped");
    assert_eq!(stub.config_writes(), writes_before, "無變更不寫檔");
    assert!(
        !stub.log().contains(&"subnet4-update".to_string()),
        "無差異不送更新"
    );
    assert_eq!(report["subnets"][0]["pool_added"], 0);
    assert_eq!(report["subnets"][0]["pool_deleted"], 0);
    assert_eq!(report["subnets"][0]["gateway_updated"], false);
}

#[tokio::test]
async fn subnet_update_failure_is_reported_without_blocking_reservations() {
    let (state, stub) = test_state().await;
    let subnet = settings_fixture(
        &state,
        &stub,
        Some("10.0.0.1"),
        &[("10.0.0.30", "10.0.0.40")],
    )
    .await;
    let asset = create_asset(&state, "同步主機").await;
    let interface = create_interface(&state, asset, "aa:aa:aa:aa:aa:01").await;
    let (status, body) =
        put_assignment(&state, subnet, "10.0.0.5", interface, "reservation", None).await;
    assert_eq!(status, StatusCode::OK, "指派應成功：{body}");
    stub.clear_hosts();
    stub.fail_commands(&["subnet4-update"]);

    let writes_before = stub.config_writes();
    let (status, report) = send(&state, Method::POST, "/api/v1/kea/sync", None).await;

    assert_eq!(status, StatusCode::OK, "套用應成功：{report}");
    let entry = &report["subnets"][0];
    assert_eq!(entry["added"], 1, "保留仍推送");
    assert_eq!(entry["pool_added"], 0, "失敗不計數");
    assert!(
        entry["settings_error"]
            .as_str()
            .is_some_and(|message| message.contains("simulated failure")),
        "網段層失敗應記錄：{entry}"
    );
    assert_eq!(report["config_write"], "ok", "保留成功仍寫檔");
    assert_eq!(stub.config_writes(), writes_before + 1, "整批一次");
    assert!(
        stub.hosts(1)
            .iter()
            .any(|host| host.ip_address == "10.0.0.5"),
        "保留已推送"
    );
}

#[tokio::test]
async fn reservations_and_settings_share_single_config_write() {
    let (state, stub) = test_state().await;
    let subnet = settings_fixture(
        &state,
        &stub,
        Some("10.0.0.1"),
        &[("10.0.0.30", "10.0.0.40")],
    )
    .await;
    let asset = create_asset(&state, "同步主機").await;
    let interface = create_interface(&state, asset, "aa:aa:aa:aa:aa:01").await;
    let (status, body) =
        put_assignment(&state, subnet, "10.0.0.5", interface, "reservation", None).await;
    assert_eq!(status, StatusCode::OK, "指派應成功：{body}");
    stub.clear_hosts();

    let writes_before = stub.config_writes();
    let (status, report) = send(&state, Method::POST, "/api/v1/kea/sync", None).await;

    assert_eq!(status, StatusCode::OK, "套用應成功：{report}");
    let entry = &report["subnets"][0];
    assert_eq!(entry["added"], 1);
    assert_eq!(entry["pool_added"], 0, "同範圍不重建");
    assert_eq!(entry["pool_deleted"], 1);
    assert_eq!(entry["gateway_updated"], true);
    assert_eq!(report["config_write"], "ok");
    assert_eq!(
        stub.config_writes(),
        writes_before + 1,
        "保留與網段層合併一次寫檔"
    );

    assert!(
        stub.hosts(1)
            .iter()
            .any(|host| host.ip_address == "10.0.0.5"),
        "保留已推送"
    );
    let options = stub.subnet(1)["option-data"].clone();
    let routers = options
        .as_array()
        .expect("option-data")
        .iter()
        .find(|option| option["name"] == "routers")
        .cloned()
        .expect("routers 存在");
    assert_eq!(routers["data"], "10.0.0.1", "gateway 已更新");
}

#[tokio::test]
async fn plan_without_kea_configuration_is_rejected() {
    let state = AppState::new(test_pool().await, dist_dir());

    let (status, body) = send(&state, Method::GET, "/api/v1/kea/sync/plan", None).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body["message"]
            .as_str()
            .is_some_and(|message| message.contains("KEA_API_URL")),
        "應提示未設定 Kea：{body}"
    );
}
