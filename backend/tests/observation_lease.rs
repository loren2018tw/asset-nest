//! Kea 租約來源整合測試（見票 03、spec §掃描服務、ADR-0015）。
//!
//! 以本機 stub Kea＋stub 探測邊界驗證：目前有效（`state=default`）租約位址
//! 納入探測目標、`cltt` 記為最後可見（來源 `kea_lease`）、與 ARP 的
//! latest-wins 語意，以及 Kea 未設定／失敗時的行為。

use std::net::Ipv4Addr;
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::extract::State;
use axum::http::{Method, Request, StatusCode, header};
use axum::routing::post;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use sqlx::SqlitePool;
use sqlx::sqlite::SqlitePoolOptions;
use tower::ServiceExt;

use asset_nest::kea::http::Client;
use asset_nest::probe::Prober;
use asset_nest::subnets::Subnet;
use asset_nest::{AppState, app, observation};

// ---------- stub Kea（只實作租約讀取） ----------

/// 記憶體版假 Kea：`lease4-get-all` 回設定租約；可切換為命令失敗。
#[derive(Clone, Default)]
struct StubKea {
    leases: Arc<Mutex<Vec<Value>>>,
    fail: Arc<Mutex<bool>>,
    log: Arc<Mutex<Vec<String>>>,
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

    fn set_leases(&self, leases: &[Value]) {
        *self.leases.lock().expect("leases") = leases.to_vec();
    }

    fn fail(&self) {
        *self.fail.lock().expect("fail") = true;
    }

    fn log(&self) -> Vec<String> {
        self.log.lock().expect("log").clone()
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

    if *stub.fail.lock().expect("fail") {
        return Json(json!([{ "result": 1, "text": "simulated failure" }]));
    }

    let response = match command.as_str() {
        "lease4-get-all" => {
            let leases = stub.leases.lock().expect("leases").clone();
            json!([{ "result": 0, "arguments": { "leases": leases } }])
        }
        other => json!([{ "result": 2, "text": format!("unsupported: {other}") }]),
    };

    Json(response)
}

// ---------- stub 探測邊界 ----------

/// 可腳本化的探測邊界 stub：記錄每次探測的目標，回傳腳本中的回應。
struct StubProber {
    local_cidrs: Vec<String>,
    responses: Mutex<Vec<(Ipv4Addr, String)>>,
    calls: Mutex<Vec<Vec<Ipv4Addr>>>,
}

impl StubProber {
    fn new(local_cidrs: &[&str]) -> Self {
        Self {
            local_cidrs: local_cidrs.iter().map(|cidr| cidr.to_string()).collect(),
            responses: Mutex::new(Vec::new()),
            calls: Mutex::new(Vec::new()),
        }
    }

    fn set_responses(&self, responses: &[(&str, &str)]) {
        *self.responses.lock().expect("stub 鎖") = responses
            .iter()
            .map(|(address, mac)| (address.parse().expect("合法位址"), mac.to_string()))
            .collect();
    }

    fn calls(&self) -> Vec<Vec<Ipv4Addr>> {
        self.calls.lock().expect("stub 鎖").clone()
    }
}

impl Prober for StubProber {
    fn is_local(&self, subnet: &Subnet) -> bool {
        self.local_cidrs.iter().any(|cidr| cidr == &subnet.cidr)
    }

    fn probe(&self, _subnet: &Subnet, targets: &[Ipv4Addr]) -> Vec<(Ipv4Addr, String)> {
        self.calls.lock().expect("stub 鎖").push(targets.to_vec());
        self.responses
            .lock()
            .expect("stub 鎖")
            .iter()
            .filter(|(address, _)| targets.contains(address))
            .cloned()
            .collect()
    }

    /// 本檔不測被動監聽；固定回空（被動語意見 `observation_passive.rs`）。
    fn passive_observe(
        &self,
        _subnet: &Subnet,
        _window: std::time::Duration,
    ) -> Vec<(Ipv4Addr, String)> {
        Vec::new()
    }
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

/// 建立附掛 stub 探測邊界的 AppState（未附掛 Kea）。
fn test_state(pool: &SqlitePool, prober: Arc<StubProber>) -> AppState {
    AppState::new(
        pool.clone(),
        std::env::temp_dir().join("asset-nest-test-no-dist"),
    )
    .with_prober(prober)
}

/// 固定時間字串 → epoch 秒（租約 `cltt` 用）。
fn epoch(rfc3339: &str) -> i64 {
    DateTime::parse_from_rfc3339(rfc3339)
        .expect("固定時間")
        .timestamp()
}

/// 一筆本網段有效租約的 JSON。
fn default_lease(address: &str, mac: &str, cltt: &str) -> Value {
    json!({
        "ip-address": address,
        "hw-address": mac,
        "subnet-id": 1,
        "cltt": epoch(cltt),
        "state": 0,
    })
}

/// 以 `oneshot` 發送請求；回傳狀態碼與 JSON（204 等空內容為 `Value::Null`）。
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

/// 新增網段並斷言成功，回傳回應 JSON。
async fn create_subnet(state: &AppState, body: Value) -> Value {
    let (status, json) = send(state, Method::POST, "/api/v1/subnets", Some(body)).await;
    assert_eq!(status, StatusCode::CREATED, "新增網段應成功：{json}");
    json
}

/// 開啟網段的快速掃描觀測。
async fn enable_observation(state: &AppState, id: i64) {
    let (status, json) = send(
        state,
        Method::PATCH,
        &format!("/api/v1/subnets/{id}"),
        Some(json!({ "observed": true })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "開啟觀測應成功：{json}");
}

/// 新增資產並斷言成功，回傳 id。
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

/// 指派位址並斷言成功。
async fn assign_ip(state: &AppState, subnet_id: i64, address: &str, body: Value) {
    let (status, json) = send(
        state,
        Method::PUT,
        &format!("/api/v1/subnets/{subnet_id}/ips/{address}/assignment"),
        Some(body),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "指派應成功：{json}");
}

/// 發送快速掃描並斷言成功，回傳報告。
async fn quick_sweep(state: &AppState, id: i64) -> Value {
    let (status, json) = send(
        state,
        Method::POST,
        &format!("/api/v1/subnets/{id}/sweeps"),
        Some(json!({ "mode": "quick" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "快速掃描應成功：{json}");
    json
}

/// 以固定 `now` 直接呼叫掃描服務（租約 `cltt` 與 ARP 的新舊比較須可控制時鐘）。
async fn quick_sweep_at(state: &AppState, subnet_id: i64, now: &str) -> Value {
    let subnet = asset_nest::subnets::get(&state.db, subnet_id)
        .await
        .expect("讀取網段")
        .expect("網段存在");
    let now = DateTime::parse_from_rfc3339(now)
        .expect("固定時間")
        .with_timezone(&Utc);

    let report = observation::run_quick(
        &state.db,
        state.prober.clone(),
        state.kea.as_ref(),
        &subnet,
        now,
    )
    .await
    .expect("快速掃描");

    serde_json::to_value(report).expect("報告轉 JSON")
}

/// 讀取某網段的 IP 清單。
async fn list_ips(state: &AppState, id: i64) -> Value {
    let (status, json) = send(
        state,
        Method::GET,
        &format!("/api/v1/subnets/{id}/ips"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "讀取 IP 清單應成功：{json}");
    json
}

/// 取出 IP 清單中某位址的列。
fn row<'a>(page: &'a Value, address: &str) -> &'a Value {
    page["items"]
        .as_array()
        .expect("items 為陣列")
        .iter()
        .find(|item| item["address"] == address)
        .unwrap_or_else(|| panic!("清單含 {address}"))
}

/// 讀取單一現況列（無列為 `None`）：最後可見時間、MAC、來源、最後檢查時間。
async fn presence(
    pool: &SqlitePool,
    subnet_id: i64,
    address: &str,
) -> Option<(
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
)> {
    sqlx::query_as(
        "SELECT last_seen_at, last_seen_mac, last_seen_source, last_checked_at
           FROM ip_presence WHERE subnet_id = ? AND address = ?",
    )
    .bind(subnet_id)
    .bind(address)
    .fetch_optional(pool)
    .await
    .expect("讀取現況列")
}

/// 讀取 `observation_event` 的（位址、MAC、種類、來源），依寫入順序。
async fn events(pool: &SqlitePool) -> Vec<(String, String, String, String)> {
    sqlx::query_as(
        "SELECT address, COALESCE(mac, ''), kind, source FROM observation_event ORDER BY id ASC",
    )
    .fetch_all(pool)
    .await
    .expect("讀取觀測事件")
}

/// 指派一個 `10.0.0.1` 的 static 位址（供目標集合與 ARP 測試）。
async fn assign_first_address(state: &AppState, subnet_id: i64) {
    let asset = create_asset(state, "主機").await;
    let interface = create_interface(
        state,
        asset,
        json!({ "name": "eth0", "mac": "aa:bb:cc:dd:ee:01" }),
    )
    .await;
    assign_ip(
        state,
        subnet_id,
        "10.0.0.1",
        json!({ "interface_id": interface, "purpose": "static" }),
    )
    .await;
}

// ---------- 測試 ----------

/// 租約位址納入探測目標與現況；非 `default` 狀態／不對應網段／非 IPv4 一律略過。
#[tokio::test]
async fn lease_addresses_join_targets_and_only_defaults_are_recorded() {
    let pool = test_pool().await;
    let prober = Arc::new(StubProber::new(&["10.0.0.0/28"]));
    let kea = StubKea::default();
    let url = kea.spawn().await;
    let state = test_state(&pool, prober.clone()).with_kea(Client::new(url.parse().expect("URL")));

    let subnet = create_subnet(&state, json!({ "cidr": "10.0.0.0/28", "kea_subnet_id": 1 })).await;
    let id = subnet["id"].as_i64().expect("回應含 id");
    enable_observation(&state, id).await;
    assign_first_address(&state, id).await;

    kea.set_leases(&[
        // 有效租約：帶 MAC。
        json!({
            "ip-address": "10.0.0.5",
            "hw-address": "AA:BB:CC:DD:EE:05",
            "subnet-id": 1,
            "cltt": epoch("2026-10-05T12:00:00Z"),
            "state": 0,
        }),
        // 有效租約但缺 hw-address：記時間與來源、不寫事件、MAC 保持 NULL。
        json!({
            "ip-address": "10.0.0.3",
            "subnet-id": 1,
            "cltt": epoch("2026-10-05T11:00:00Z"),
            "state": "default",
        }),
        // 有效租約但缺 cltt：仍納入探測目標，但無法記「最後可見」。
        json!({
            "ip-address": "10.0.0.10",
            "subnet-id": 1,
            "state": "default",
        }),
        // 以下皆非目前有效租約，不得納入目標或現況。
        json!({
            "ip-address": "10.0.0.4",
            "subnet-id": 2,
            "cltt": epoch("2026-10-05T12:00:00Z"),
            "state": "default",
        }),
        json!({
            "ip-address": "10.0.0.6",
            "subnet-id": 1,
            "cltt": epoch("2026-10-05T12:00:00Z"),
            "state": 1,
        }),
        json!({
            "ip-address": "10.0.0.7",
            "subnet-id": 1,
            "cltt": epoch("2026-10-05T12:00:00Z"),
            "state": 2,
        }),
        json!({
            "ip-address": "10.0.0.8",
            "subnet-id": 1,
            "cltt": epoch("2026-10-05T12:00:00Z"),
            "state": 3,
        }),
        json!({
            "ip-address": "10.0.0.9",
            "subnet-id": 1,
            "cltt": epoch("2026-10-05T12:00:00Z"),
        }),
        json!({
            "ip-address": "fd42::5",
            "subnet-id": 1,
            "cltt": epoch("2026-10-05T12:00:00Z"),
            "state": "default",
        }),
    ]);

    let report = quick_sweep(&state, id).await;
    assert_eq!(report["mode"], "quick");
    assert_eq!(
        report["targets"], 4,
        "目標＝已指派 ∪ 有效租約（含缺 cltt 者）：{report}"
    );
    assert_eq!(report["seen"], 2, "seen＝ARP 或有效租約（有 cltt）");

    assert_eq!(
        prober.calls(),
        vec![vec![
            "10.0.0.1".parse::<Ipv4Addr>().expect("指派位址"),
            "10.0.0.3".parse::<Ipv4Addr>().expect("租約位址"),
            "10.0.0.5".parse::<Ipv4Addr>().expect("租約位址"),
            "10.0.0.10".parse::<Ipv4Addr>().expect("租約位址"),
        ]],
        "租約位址須併入探測目標（數值升冪）"
    );

    // 帶 MAC 的有效租約：cltt 記為最後可見、來源 kea_lease、MAC 正規化。
    let page = list_ips(&state, id).await;
    let with_mac = row(&page, "10.0.0.5");
    assert_eq!(with_mac["last_seen_at"], "2026-10-05T12:00:00Z");
    assert_eq!(with_mac["last_seen_mac"], "aa:bb:cc:dd:ee:05");
    assert_eq!(with_mac["last_seen_source"], "kea_lease");
    assert!(
        with_mac["last_checked_at"].is_string(),
        "租約目標亦更新 last_checked_at：{with_mac}"
    );

    // 缺 MAC 的有效租約：仍記最後可見與來源，MAC 為 null。
    let without_mac = row(&page, "10.0.0.3");
    assert_eq!(without_mac["last_seen_at"], "2026-10-05T11:00:00Z");
    assert_eq!(without_mac["last_seen_mac"], Value::Null);
    assert_eq!(without_mac["last_seen_source"], "kea_lease");

    // 缺 cltt：僅更新 last_checked_at，不記最後可見。
    let no_cltt = row(&page, "10.0.0.10");
    assert!(no_cltt["last_seen_at"].is_null());
    assert!(no_cltt["last_seen_source"].is_null());
    assert!(no_cltt["last_checked_at"].is_string());

    // 非 default／不對應網段／非 IPv4：完全不得留下現況列。
    for ignored in ["10.0.0.4", "10.0.0.6", "10.0.0.7", "10.0.0.8", "10.0.0.9"] {
        assert!(
            presence(&pool, id, ignored).await.is_none(),
            "{ignored} 不得留下現況列"
        );
    }

    // 事件：只有帶 MAC 的有效租約寫 first_seen（來源 kea_lease）。
    assert_eq!(
        events(&pool).await,
        vec![(
            "10.0.0.5".to_string(),
            "aa:bb:cc:dd:ee:05".to_string(),
            "first_seen".to_string(),
            "kea_lease".to_string(),
        )]
    );
}

/// 同一位址的 ARP 與租約取最近者；較新的訊號連來源一起勝出。
#[tokio::test]
async fn latest_signal_wins_between_arp_and_lease() {
    let pool = test_pool().await;
    let prober = Arc::new(StubProber::new(&["10.0.0.0/28"]));
    let kea = StubKea::default();
    let url = kea.spawn().await;
    let state = test_state(&pool, prober.clone()).with_kea(Client::new(url.parse().expect("URL")));

    let subnet = create_subnet(&state, json!({ "cidr": "10.0.0.0/28", "kea_subnet_id": 1 })).await;
    let id = subnet["id"].as_i64().expect("回應含 id");
    enable_observation(&state, id).await;

    // 租約較舊（10-04）、ARP 當下回應（now=10-05 00:00）→ ARP 勝、來源 arp。
    kea.set_leases(&[default_lease(
        "10.0.0.5",
        "aa:bb:cc:dd:ee:05",
        "2026-10-04T00:00:00Z",
    )]);
    prober.set_responses(&[("10.0.0.5", "aa:bb:cc:dd:ee:05")]);
    quick_sweep_at(&state, id, "2026-10-05T00:00:00Z").await;

    let page = list_ips(&state, id).await;
    let seen = row(&page, "10.0.0.5");
    assert_eq!(seen["last_seen_source"], "arp", "較新的 ARP 勝出：{seen}");
    assert_eq!(seen["last_seen_at"], "2026-10-05T00:00:00Z");
    assert_eq!(seen["last_checked_at"], "2026-10-05T00:00:00Z");

    // 租約較新（10-06 00:00，now=10-06 01:00）、ARP 無回應 → 租約勝、時間 cltt。
    prober.set_responses(&[]);
    kea.set_leases(&[default_lease(
        "10.0.0.5",
        "aa:bb:cc:dd:ee:05",
        "2026-10-06T00:00:00Z",
    )]);
    let report = quick_sweep_at(&state, id, "2026-10-06T01:00:00Z").await;
    assert_eq!(report["seen"], 1, "租約仍為本輪證據：{report}");

    let page = list_ips(&state, id).await;
    let seen = row(&page, "10.0.0.5");
    assert_eq!(
        seen["last_seen_source"], "kea_lease",
        "較新的租約 cltt 勝過舊 ARP：{seen}"
    );
    assert_eq!(seen["last_seen_at"], "2026-10-06T00:00:00Z");
    assert_eq!(
        seen["last_checked_at"], "2026-10-06T01:00:00Z",
        "last_checked_at 仍為本輪 now"
    );

    // 租約換 MAC（now=10-07 01:00）→ mac_changed 事件、來源 kea_lease。
    prober.set_responses(&[]);
    kea.set_leases(&[default_lease(
        "10.0.0.5",
        "AA:BB:CC:DD:EE:99",
        "2026-10-07T00:00:00Z",
    )]);
    quick_sweep_at(&state, id, "2026-10-07T01:00:00Z").await;

    let page = list_ips(&state, id).await;
    let seen = row(&page, "10.0.0.5");
    assert_eq!(
        seen["last_seen_mac"], "aa:bb:cc:dd:ee:99",
        "租約 MAC 正規化"
    );
    assert_eq!(seen["last_seen_source"], "kea_lease");

    let events = events(&pool).await;
    assert_eq!(
        events.len(),
        2,
        "同 MAC 不重複寫事件，換 MAC 才寫：{events:?}"
    );
    assert_eq!(
        events[1],
        (
            "10.0.0.5".to_string(),
            "aa:bb:cc:dd:ee:99".to_string(),
            "mac_changed".to_string(),
            "kea_lease".to_string(),
        )
    );
}

/// 較舊租約的 MAC 不得每輪以回溯時間重複寫事件（複查修正）：ARP 當下
/// （MAC A）與較舊 `cltt` 租約（MAC B）並存時，重跑同一輪不得新增事件。
#[tokio::test]
async fn stale_lease_mac_does_not_rewrite_events_on_repeated_sweeps() {
    let pool = test_pool().await;
    let prober = Arc::new(StubProber::new(&["10.0.0.0/28"]));
    let kea = StubKea::default();
    let url = kea.spawn().await;
    let state = test_state(&pool, prober.clone()).with_kea(Client::new(url.parse().expect("URL")));

    let subnet = create_subnet(&state, json!({ "cidr": "10.0.0.0/28", "kea_subnet_id": 1 })).await;
    let id = subnet["id"].as_i64().expect("回應含 id");
    enable_observation(&state, id).await;

    // 租約較舊（cltt 10-04）且 MAC B；ARP 當下（now=10-05）回 MAC A。
    kea.set_leases(&[default_lease(
        "10.0.0.5",
        "aa:bb:cc:dd:ee:05",
        "2026-10-04T00:00:00Z",
    )]);
    prober.set_responses(&[("10.0.0.5", "aa:bb:cc:dd:ee:99")]);

    // 第一輪：租約先寫 first_seen（回溯 cltt），ARP 後寫 mac_changed（當下）。
    quick_sweep_at(&state, id, "2026-10-05T00:00:00Z").await;
    let first = events(&pool).await;
    assert_eq!(
        first,
        vec![
            (
                "10.0.0.5".to_string(),
                "aa:bb:cc:dd:ee:05".to_string(),
                "first_seen".to_string(),
                "kea_lease".to_string(),
            ),
            (
                "10.0.0.5".to_string(),
                "aa:bb:cc:dd:ee:99".to_string(),
                "mac_changed".to_string(),
                "arp".to_string(),
            ),
        ],
        "首輪＝租約 first_seen＋ARP mac_changed：{first:?}"
    );

    let page = list_ips(&state, id).await;
    let seen = row(&page, "10.0.0.5");
    assert_eq!(seen["last_seen_at"], "2026-10-05T00:00:00Z");
    assert_eq!(seen["last_seen_mac"], "aa:bb:cc:dd:ee:99");
    assert_eq!(seen["last_seen_source"], "arp", "較新的 ARP 勝出：{seen}");

    // 第二輪（同一 now 與租約）：舊租約不得再寫回溯事件，ARP 同 MAC 亦不寫。
    quick_sweep_at(&state, id, "2026-10-05T00:00:00Z").await;
    assert_eq!(
        events(&pool).await.len(),
        2,
        "重跑同一輪後事件數不變（舊租約不得每輪重寫）"
    );

    let page = list_ips(&state, id).await;
    let seen = row(&page, "10.0.0.5");
    assert_eq!(
        seen["last_seen_mac"], "aa:bb:cc:dd:ee:99",
        "現況仍為 ARP 的 MAC A"
    );
    assert_eq!(seen["last_seen_source"], "arp");
}

/// Kea 未設定：行為與過去相同，不讀租約、不留下租約現況。
#[tokio::test]
async fn without_kea_leases_are_ignored_and_no_command_is_sent() {
    let pool = test_pool().await;
    let prober = Arc::new(StubProber::new(&["10.0.0.0/28"]));
    let kea = StubKea::default();
    let _url = kea.spawn().await;
    kea.set_leases(&[default_lease(
        "10.0.0.5",
        "aa:bb:cc:dd:ee:05",
        "2026-10-05T12:00:00Z",
    )]);

    // stub Kea 有啟動但未附掛：不得發送任何命令、不得使用租約。
    let state = test_state(&pool, prober.clone());
    let subnet = create_subnet(&state, json!({ "cidr": "10.0.0.0/28", "kea_subnet_id": 1 })).await;
    let id = subnet["id"].as_i64().expect("回應含 id");
    enable_observation(&state, id).await;
    assign_first_address(&state, id).await;

    let report = quick_sweep(&state, id).await;
    assert_eq!(report["targets"], 1, "未設定 Kea 時僅掃描已指派位址");
    assert_eq!(report["seen"], 0);
    assert_eq!(
        prober.calls(),
        vec![vec!["10.0.0.1".parse::<Ipv4Addr>().expect("指派位址")]],
        "不得把租約位址併入目標"
    );
    assert!(kea.log().is_empty(), "未設定時不得發送任何命令");
    assert!(
        presence(&pool, id, "10.0.0.5").await.is_none(),
        "不得留下租約現況列"
    );
}

/// Kea 讀取失敗：記警告但掃描照常成功，沿用 ARP 目標。
#[tokio::test]
async fn kea_failure_keeps_arp_sweep_successful() {
    let pool = test_pool().await;
    let prober = Arc::new(StubProber::new(&["10.0.0.0/28"]));
    let kea = StubKea::default();
    let url = kea.spawn().await;
    let state = test_state(&pool, prober.clone()).with_kea(Client::new(url.parse().expect("URL")));

    let subnet = create_subnet(&state, json!({ "cidr": "10.0.0.0/28", "kea_subnet_id": 1 })).await;
    let id = subnet["id"].as_i64().expect("回應含 id");
    enable_observation(&state, id).await;
    assign_first_address(&state, id).await;

    kea.fail();
    prober.set_responses(&[("10.0.0.1", "aa:bb:cc:dd:ee:01")]);

    let report = quick_sweep(&state, id).await;
    assert_eq!(report["targets"], 1, "失敗時僅剩已指派目標：{report}");
    assert_eq!(report["seen"], 1, "ARP 仍照常記錄：{report}");

    let page = list_ips(&state, id).await;
    assert_eq!(row(&page, "10.0.0.1")["last_seen_source"], "arp");
    assert_eq!(
        kea.log(),
        vec!["lease4-get-all".to_string()],
        "仍嘗試讀取租約，但失敗不影響掃描"
    );
}
