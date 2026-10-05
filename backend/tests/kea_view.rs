//! Kea 系統狀態端點整合測試：以本機 stub Kea 驗證 `GET /api/v1/kea/status`
//! 的未設定、正常與分區容錯行為（見 `.scratch/kea-pages/spec.md`）。
//!
//! stub 只實作唯讀命令（version-get／config-get／status-get），資料存記憶體、
//! 不觸及真機；真機唯讀實測見 `tests/kea_connectivity.rs`。

use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::extract::State;
use axum::http::{Method, Request, StatusCode};
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

/// 記憶體版假 Kea（唯讀命令子集）；`clone` 共用同一份狀態。
#[derive(Clone)]
struct StubKea {
    interfaces: Arc<Mutex<Vec<String>>>,
    lease_backend: Arc<Mutex<String>>,
    subnets: Arc<Mutex<Vec<(i64, String)>>>,
    status: Arc<Mutex<Value>>,
    fail: Arc<Mutex<Vec<String>>>,
    log: Arc<Mutex<Vec<String>>>,
    requests: Arc<Mutex<Vec<Value>>>,
}

impl Default for StubKea {
    fn default() -> Self {
        Self {
            interfaces: Arc::new(Mutex::new(vec!["eth0".to_string()])),
            lease_backend: Arc::new(Mutex::new("memfile".to_string())),
            subnets: Arc::new(Mutex::new(vec![(1, "10.0.0.0/24".to_string())])),
            status: Arc::new(Mutex::new(json!({
                "pid": 123,
                "uptime": 456,
                "reload": 789,
                "sockets": { "status": "ready" },
            }))),
            fail: Arc::new(Mutex::new(Vec::new())),
            log: Arc::new(Mutex::new(Vec::new())),
            requests: Arc::new(Mutex::new(Vec::new())),
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

    fn requests(&self) -> Vec<Value> {
        self.requests.lock().expect("requests").clone()
    }

    fn fail_commands(&self, commands: &[&str]) {
        *self.fail.lock().expect("fail") = commands.iter().map(|c| c.to_string()).collect();
    }

    fn set_interfaces(&self, interfaces: &[&str]) {
        *self.interfaces.lock().expect("interfaces") =
            interfaces.iter().map(|value| value.to_string()).collect();
    }

    fn set_subnets(&self, subnets: &[(i64, &str)]) {
        *self.subnets.lock().expect("subnets") = subnets
            .iter()
            .map(|(id, cidr)| (*id, cidr.to_string()))
            .collect();
    }

    fn set_status(&self, status: Value) {
        *self.status.lock().expect("status") = status;
    }
}

/// stub 的命令處理：回應格式比照 Kea（單元素陣列、result 0／1）。
async fn handle(State(stub): State<StubKea>, Json(payload): Json<Value>) -> Json<Value> {
    let command = payload
        .get("command")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    stub.log.lock().expect("log").push(command.clone());
    stub.requests.lock().expect("requests").push(payload);

    if stub
        .fail
        .lock()
        .expect("fail")
        .iter()
        .any(|failed| failed == &command)
    {
        return Json(json!([{ "result": 1, "text": "simulated failure" }]));
    }

    let response = match command.as_str() {
        "version-get" => json!([{
            "result": 0,
            "text": "Kea",
            "arguments": { "version": "3.2.1" }
        }]),
        "config-get" => {
            let interfaces = stub.interfaces.lock().expect("interfaces").clone();
            let lease_backend = stub.lease_backend.lock().expect("lease_backend").clone();
            let subnets: Vec<Value> = stub
                .subnets
                .lock()
                .expect("subnets")
                .iter()
                .map(|(id, cidr)| json!({ "id": id, "subnet": cidr }))
                .collect();
            json!([{
                "result": 0,
                "arguments": { "Dhcp4": {
                    "interfaces-config": { "interfaces": interfaces },
                    "lease-database": { "type": lease_backend },
                    "subnet4": subnets,
                } }
            }])
        }
        "status-get" => {
            let arguments = stub.status.lock().expect("status").clone();
            json!([{ "result": 0, "arguments": arguments }])
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

/// 建立附掛 stub Kea 的 AppState，並回傳 stub 基底 URL。
async fn test_state() -> (AppState, StubKea, String) {
    let stub = StubKea::default();
    let url = stub.spawn().await;
    let state = AppState::new(test_pool().await, dist_dir())
        .with_kea(Client::new(url.parse().expect("stub URL")));
    (state, stub, url)
}

/// 以 `oneshot` 發送 `GET /api/v1/kea/status`；回傳狀態碼與 JSON。
async fn get_status(state: &AppState) -> (StatusCode, Value) {
    let request = Request::builder()
        .method(Method::GET)
        .uri("/api/v1/kea/status")
        .body(Body::empty())
        .expect("建立請求");

    let response = app(state.clone()).oneshot(request).await.expect("執行請求");

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

/// 直接寫入本地網段（帶／不帶 `kea_subnet_id`）。
async fn insert_subnet(state: &AppState, cidr: &str, kea_subnet_id: Option<i64>) {
    sqlx::query("INSERT INTO subnets (cidr, kea_subnet_id) VALUES (?, ?)")
        .bind(cidr)
        .bind(kea_subnet_id)
        .execute(&state.db)
        .await
        .expect("新增測試網段");
}

// ---------- 測試 ----------

#[tokio::test]
async fn status_without_kea_is_neutral_and_sends_no_command() {
    // stub 有啟動但不附掛：驗證未設定時不發任何命令。
    let stub = StubKea::default();
    let _url = stub.spawn().await;
    let state = AppState::new(test_pool().await, dist_dir());

    let (status, body) = get_status(&state).await;

    assert_eq!(status, StatusCode::OK, "未設定也應回 200：{body}");
    assert_eq!(body["configured"], false);
    assert_eq!(body["reachable"], false);
    assert!(body["url"].is_null());
    assert!(body["version"].is_null());
    assert!(body["interfaces"].is_null());
    assert!(body["runtime"].is_null());
    assert!(body["dhcp4"].is_null());
    assert!(body.get("errors").is_none(), "無失敗時省略 errors：{body}");
    assert!(stub.log().is_empty(), "未設定時不得發送任何命令");
}

#[tokio::test]
async fn status_reports_all_blocks_and_managed_subnet_count() {
    let (state, stub, url) = test_state().await;
    stub.set_interfaces(&["eth0", "eth1"]);
    stub.set_subnets(&[
        (1, "10.0.0.0/24"),
        (2, "10.1.0.0/16"),
        (7, "192.168.0.0/24"),
    ]);
    stub.set_status(json!({
        "pid": 4321,
        "uptime": 120,
        "reload": 60,
        "sockets": { "status": "ready" },
    }));
    insert_subnet(&state, "10.0.0.0/24", Some(1)).await;
    insert_subnet(&state, "10.9.0.0/24", None).await;

    let (status, body) = get_status(&state).await;

    assert_eq!(status, StatusCode::OK, "狀態端點一律 200：{body}");
    assert_eq!(body["configured"], true);
    assert_eq!(body["reachable"], true);
    assert_eq!(body["url"], url.as_str());
    assert_eq!(body["version"]["version"], "3.2.1");
    assert_eq!(body["version"]["text"], "Kea");
    assert_eq!(body["interfaces"], json!(["eth0", "eth1"]));
    assert_eq!(body["runtime"]["pid"], 4321);
    assert_eq!(body["runtime"]["uptime"], 120);
    assert_eq!(body["runtime"]["reload"], 60);
    assert_eq!(body["runtime"]["sockets"]["status"], "ready");
    assert_eq!(body["dhcp4"]["subnet_count"], 3);
    assert_eq!(
        body["dhcp4"]["managed_subnet_count"], 1,
        "僅算帶 kea_subnet_id 者"
    );
    assert_eq!(body["dhcp4"]["lease_backend"], "memfile");
    assert!(body.get("errors").is_none(), "正常時省略 errors：{body}");

    let mut commands = stub.log();
    commands.sort();
    assert_eq!(
        commands,
        vec![
            "config-get".to_string(),
            "status-get".to_string(),
            "version-get".to_string(),
        ],
        "只送三個唯讀命令"
    );
    for request in stub.requests() {
        assert!(
            request.get("arguments").is_none(),
            "唯讀命令不帶 arguments（含 operation-target）：{request}"
        );
    }
}

#[tokio::test]
async fn status_partial_failure_keeps_successful_blocks() {
    let (state, stub, _url) = test_state().await;
    stub.fail_commands(&["config-get", "status-get"]);
    insert_subnet(&state, "10.0.0.0/24", Some(1)).await;

    let (status, body) = get_status(&state).await;

    assert_eq!(status, StatusCode::OK, "命令失敗仍應 200：{body}");
    assert_eq!(body["configured"], true);
    assert_eq!(body["reachable"], true, "version-get 成功即視為可達");
    assert_eq!(body["version"]["version"], "3.2.1");
    assert!(
        body["interfaces"].is_null(),
        "config 失敗 → interfaces null"
    );
    assert!(body["runtime"].is_null(), "status 失敗 → runtime null");
    assert!(body["dhcp4"].is_null(), "config 失敗 → dhcp4 null");
    assert!(body["errors"]["version"].is_null());
    assert!(
        body["errors"]["config"]
            .as_str()
            .is_some_and(|message| message.contains("simulated failure")),
        "errors.config 記錄失敗原因：{body}"
    );
    assert!(
        body["errors"]["status"]
            .as_str()
            .is_some_and(|message| message.contains("simulated failure")),
        "errors.status 記錄失敗原因：{body}"
    );
}

#[tokio::test]
async fn status_version_failure_marks_unreachable_but_keeps_other_blocks() {
    let (state, stub, _url) = test_state().await;
    stub.fail_commands(&["version-get"]);
    insert_subnet(&state, "10.0.0.0/24", Some(1)).await;

    let (status, body) = get_status(&state).await;

    assert_eq!(status, StatusCode::OK, "命令失敗仍應 200：{body}");
    assert_eq!(body["reachable"], false, "version-get 失敗即視為不可達");
    assert!(body["version"].is_null());
    assert!(
        body["errors"]["version"]
            .as_str()
            .is_some_and(|message| message.contains("simulated failure")),
        "errors.version 記錄失敗原因：{body}"
    );
    assert_eq!(body["interfaces"], json!(["eth0"]), "config 成功區塊保留");
    assert_eq!(body["runtime"]["pid"], 123, "status 成功區塊保留");
    assert_eq!(body["dhcp4"]["subnet_count"], 1, "config 成功區塊保留");
    assert_eq!(body["dhcp4"]["managed_subnet_count"], 1);
}

#[tokio::test]
async fn status_empty_interfaces_and_missing_fields_are_neutral() {
    let (state, stub, _url) = test_state().await;
    stub.set_interfaces(&[]);
    stub.set_subnets(&[]);
    stub.set_status(json!({}));

    let (status, body) = get_status(&state).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["interfaces"],
        json!([]),
        "空清單＝未監聽，正常回應而非錯誤"
    );
    assert!(
        body.get("errors").is_none(),
        "空 interfaces 不是錯誤：{body}"
    );
    assert_eq!(body["dhcp4"]["subnet_count"], 0);
    assert_eq!(body["dhcp4"]["managed_subnet_count"], 0);
    assert!(body["runtime"]["pid"].is_null());
    assert!(body["runtime"]["uptime"].is_null());
    assert!(body["runtime"]["reload"].is_null());
    assert!(body["runtime"]["sockets"].is_null(), "缺 sockets 為 null");
}
