//! 網段外觀測（探索時被動 ARP 監聽）整合測試（見票 01、spec §掃描服務／
//! §HTTP API、ADR-0017）。
//!
//! 以注入的 stub 探測邊界腳本化 `passive_observe` 回傳，驗證 CIDR 外寫入
//! （`out_of_subnet=1`、來源 `arp_passive`）、CIDR 內丟棄、`passive_seen`、
//! 窗長 0 停用、unprivileged 空資料與清單端點。真機短窗唯讀監聽為
//! `#[ignore]`，預設不執行（執行方式見檔尾）。

use std::net::Ipv4Addr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use sqlx::SqlitePool;
use sqlx::sqlite::SqlitePoolOptions;
use tower::ServiceExt;

use asset_nest::probe::Prober;
use asset_nest::subnets::Subnet;
use asset_nest::{AppState, app};

/// 可腳本化的探測邊界 stub：主動回應只回命中目標者；被動回傳整份腳本，
/// 並記錄每次 `passive_observe` 收到的窗長。
struct StubProber {
    local_cidrs: Vec<String>,
    responses: Mutex<Vec<(Ipv4Addr, String)>>,
    passive: Mutex<Vec<(Ipv4Addr, String)>>,
    passive_calls: Mutex<Vec<Duration>>,
}

impl StubProber {
    fn new(local_cidrs: &[&str]) -> Self {
        Self {
            local_cidrs: local_cidrs.iter().map(|cidr| cidr.to_string()).collect(),
            responses: Mutex::new(Vec::new()),
            passive: Mutex::new(Vec::new()),
            passive_calls: Mutex::new(Vec::new()),
        }
    }

    /// 設定主動回應腳本（取代先前內容）。
    fn set_responses(&self, responses: &[(&str, &str)]) {
        *self.responses.lock().expect("stub 鎖") = responses
            .iter()
            .map(|(address, mac)| (address.parse().expect("合法位址"), mac.to_string()))
            .collect();
    }

    /// 設定被動 sender 腳本（取代先前內容）。
    fn set_passive(&self, senders: &[(&str, &str)]) {
        *self.passive.lock().expect("stub 鎖") = senders
            .iter()
            .map(|(address, mac)| (address.parse().expect("合法位址"), mac.to_string()))
            .collect();
    }

    /// 每次 `passive_observe` 收到的窗長（依呼叫順序）。
    fn passive_calls(&self) -> Vec<Duration> {
        self.passive_calls.lock().expect("stub 鎖").clone()
    }
}

impl Prober for StubProber {
    fn is_local(&self, subnet: &Subnet) -> bool {
        self.local_cidrs.iter().any(|cidr| cidr == &subnet.cidr)
    }

    fn probe(&self, _subnet: &Subnet, targets: &[Ipv4Addr]) -> Vec<(Ipv4Addr, String)> {
        self.responses
            .lock()
            .expect("stub 鎖")
            .iter()
            .filter(|(address, _)| targets.contains(address))
            .cloned()
            .collect()
    }

    fn passive_observe(&self, _subnet: &Subnet, window: Duration) -> Vec<(Ipv4Addr, String)> {
        self.passive_calls.lock().expect("stub 鎖").push(window);
        self.passive.lock().expect("stub 鎖").clone()
    }
}

/// 建立測試資料庫並套用 migrations（領域測試必須先套）。
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

/// 建立附掛 stub 探測邊界的 AppState；速率上限拉大＝單一批次、不睡眠，
/// 被動窗長由測試指定。
fn test_state(pool: &SqlitePool, prober: Arc<StubProber>, passive_window_secs: u64) -> AppState {
    AppState::new(
        pool.clone(),
        std::env::temp_dir().join("asset-nest-test-no-dist"),
    )
    .with_prober(prober)
    .with_discovery_rate_pps(u32::MAX)
    .with_passive_window_secs(passive_window_secs)
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

/// 新增網段並斷言成功，回傳 id。
async fn create_subnet(state: &AppState, cidr: &str) -> i64 {
    let (status, json) = send(
        state,
        Method::POST,
        "/api/v1/subnets",
        Some(json!({ "cidr": cidr })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "新增網段應成功：{json}");
    json["id"].as_i64().expect("回應含 id")
}

/// 以 PATCH 套用欄位並斷言成功，回傳回應 JSON。
async fn patch_subnet(state: &AppState, id: i64, body: Value) -> Value {
    let (status, json) = send(
        state,
        Method::PATCH,
        &format!("/api/v1/subnets/{id}"),
        Some(body),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "PATCH 應成功：{json}");
    json
}

/// 開啟網段觀測並啟用探索掃描。
async fn enable_discovery(state: &AppState, id: i64) {
    let enabled = patch_subnet(state, id, json!({ "observed": true })).await;
    assert_eq!(enabled["observed"], true);
    let discovery = patch_subnet(state, id, json!({ "discovery_enabled": true })).await;
    assert_eq!(discovery["discovery_enabled"], true);
}

/// 發送探索掃描並斷言成功，回傳報告。
async fn discovery_sweep(state: &AppState, id: i64) -> Value {
    let (status, json) = send(
        state,
        Method::POST,
        &format!("/api/v1/subnets/{id}/sweeps"),
        Some(json!({ "mode": "discovery" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "探索掃描應成功：{json}");
    json
}

/// 讀取網段外觀測清單並斷言成功。
async fn unmanaged_list(state: &AppState) -> Value {
    let (status, json) = send(state, Method::GET, "/api/v1/observations/unmanaged", None).await;
    assert_eq!(status, StatusCode::OK, "讀取網段外觀測清單應成功：{json}");
    json
}

/// 讀取 `observation_event` 的（位址、MAC、種類、來源、時間），依寫入順序。
async fn events(pool: &SqlitePool) -> Vec<(String, String, String, String, String)> {
    sqlx::query_as(
        "SELECT address, COALESCE(mac, ''), kind, source, observed_at
           FROM observation_event ORDER BY id ASC",
    )
    .fetch_all(pool)
    .await
    .expect("讀取觀測事件")
}

/// 讀取 `ip_presence` 全部列（位址、旗標、MAC、來源、檢查時間；依位址）。
async fn presence_rows(
    pool: &SqlitePool,
) -> Vec<(String, i64, Option<String>, Option<String>, Option<String>)> {
    sqlx::query_as(
        "SELECT address, out_of_subnet, last_seen_mac, last_seen_source, last_checked_at
           FROM ip_presence ORDER BY address",
    )
    .fetch_all(pool)
    .await
    .expect("讀取觀測現況")
}

#[tokio::test]
async fn discovery_records_out_of_subnet_sender_with_flag_event_and_report() {
    let pool = test_pool().await;
    let stub = Arc::new(StubProber::new(&["10.0.0.0/29"]));
    let state = test_state(&pool, stub.clone(), 5);

    let id = create_subnet(&state, "10.0.0.0/29").await;
    enable_discovery(&state, id).await;

    // 主動無回應；被動腳本回一個 CIDR 外 sender。
    stub.set_passive(&[("10.0.9.9", "AA:BB:CC:DD:EE:99")]);
    let report = discovery_sweep(&state, id).await;

    assert_eq!(report["mode"], "discovery");
    assert_eq!(report["seen"], 0, "主動無證據");
    assert_eq!(report["passive_seen"], 1, "報告含被動看到的相異位址數");
    assert_eq!(
        stub.passive_calls(),
        vec![Duration::from_secs(5)],
        "被動監聽窗長取自 AppState"
    );

    // 現況列：旗標 1、來源 arp_passive、MAC 正規化；未被檢查（非指派目標）。
    assert_eq!(
        presence_rows(&pool).await,
        vec![(
            "10.0.9.9".to_string(),
            1,
            Some("aa:bb:cc:dd:ee:99".to_string()),
            Some("arp_passive".to_string()),
            None,
        )],
        "只有被動列且旗標為 1"
    );

    // 事件：first_seen、來源 arp_passive、時間與現況一致（UTC）。
    let recorded = events(&pool).await;
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].0, "10.0.9.9");
    assert_eq!(recorded[0].1, "aa:bb:cc:dd:ee:99");
    assert_eq!(recorded[0].2, "first_seen");
    assert_eq!(recorded[0].3, "arp_passive");
    assert!(
        recorded[0].4.ends_with('Z'),
        "事件時間為 UTC：{}",
        recorded[0].4
    );
}

#[tokio::test]
async fn discovery_discards_in_cidr_passive_senders_and_leaves_active_flag_zero() {
    let pool = test_pool().await;
    let stub = Arc::new(StubProber::new(&["10.0.0.0/29"]));
    let state = test_state(&pool, stub.clone(), 5);

    let id = create_subnet(&state, "10.0.0.0/29").await;
    enable_discovery(&state, id).await;

    // .2 在 CIDR 內（同時主動回應）；9.9 在 CIDR 外。
    stub.set_responses(&[("10.0.0.2", "aa:bb:cc:00:00:02")]);
    stub.set_passive(&[
        ("10.0.0.2", "aa:bb:cc:00:00:02"),
        ("10.0.9.9", "aa:bb:cc:dd:ee:99"),
    ]);
    let report = discovery_sweep(&state, id).await;

    assert_eq!(report["seen"], 1, "主動看到 .2");
    assert_eq!(report["passive_seen"], 1, "CIDR 內的被動 sender 不計入");
    assert_eq!(
        presence_rows(&pool).await,
        vec![
            (
                "10.0.0.2".to_string(),
                0,
                Some("aa:bb:cc:00:00:02".to_string()),
                Some("arp".to_string()),
                None,
            ),
            (
                "10.0.9.9".to_string(),
                1,
                Some("aa:bb:cc:dd:ee:99".to_string()),
                Some("arp_passive".to_string()),
                None,
            ),
        ],
        "CIDR 內由主動負責（旗標 0）、CIDR 外被動列旗標 1；未指派者不更新 last_checked_at"
    );

    // 事件：.2 由主動寫（arp）、9.9 由被動寫（arp_passive）。
    let recorded = events(&pool).await;
    assert_eq!(
        recorded
            .iter()
            .map(|(address, _, kind, source, _)| (address.as_str(), kind.as_str(), source.as_str()))
            .collect::<Vec<_>>(),
        vec![
            ("10.0.0.2", "first_seen", "arp"),
            ("10.0.9.9", "first_seen", "arp_passive"),
        ]
    );
}

#[tokio::test]
async fn repeated_discovery_same_passive_mac_writes_no_extra_events() {
    let pool = test_pool().await;
    let stub = Arc::new(StubProber::new(&["10.0.0.0/29"]));
    let state = test_state(&pool, stub.clone(), 5);

    let id = create_subnet(&state, "10.0.0.0/29").await;
    enable_discovery(&state, id).await;
    stub.set_passive(&[("10.0.9.9", "aa:bb:cc:dd:ee:99")]);

    let first = discovery_sweep(&state, id).await;
    let second = discovery_sweep(&state, id).await;

    assert_eq!(first["passive_seen"], 1);
    assert_eq!(second["passive_seen"], 1, "重複看到仍算本輪寫入的位址");
    assert_eq!(events(&pool).await.len(), 1, "同 MAC 重複探索不重複寫事件");
    assert_eq!(presence_rows(&pool).await.len(), 1);
    assert_eq!(stub.passive_calls().len(), 2, "兩輪都啟動被動監聽");
}

#[tokio::test]
async fn passive_window_zero_disables_listening() {
    let pool = test_pool().await;
    let stub = Arc::new(StubProber::new(&["10.0.0.0/29"]));
    let state = test_state(&pool, stub.clone(), 0);

    let id = create_subnet(&state, "10.0.0.0/29").await;
    enable_discovery(&state, id).await;
    stub.set_passive(&[("10.0.9.9", "aa:bb:cc:dd:ee:99")]);

    let report = discovery_sweep(&state, id).await;

    assert_eq!(report["passive_seen"], 0);
    assert!(
        stub.passive_calls().is_empty(),
        "窗長 0 時不得呼叫 passive_observe"
    );
    assert!(presence_rows(&pool).await.is_empty(), "不得寫任何現況列");
    assert!(events(&pool).await.is_empty(), "不得寫任何事件");
}

#[tokio::test]
async fn unprivileged_style_empty_passive_yields_no_rows() {
    let pool = test_pool().await;
    let stub = Arc::new(StubProber::new(&["10.0.0.0/29"]));
    let state = test_state(&pool, stub.clone(), 5);

    let id = create_subnet(&state, "10.0.0.0/29").await;
    enable_discovery(&state, id).await;

    // 模擬 unprivileged／降級：窗內回空集合。
    let report = discovery_sweep(&state, id).await;

    assert_eq!(report["passive_seen"], 0);
    assert!(presence_rows(&pool).await.is_empty());
    assert_eq!(
        unmanaged_list(&state).await["items"]
            .as_array()
            .map(Vec::len),
        Some(0)
    );
}

#[tokio::test]
async fn unmanaged_endpoint_lists_newest_first_with_subnet_and_asset() {
    let pool = test_pool().await;
    let stub = Arc::new(StubProber::new(&["10.0.0.0/29"]));
    let state = test_state(&pool, stub, 60);

    let id = create_subnet(&state, "10.0.0.0/29").await;
    let named = patch_subnet(&state, id, json!({ "name": "機房 A" })).await;
    assert_eq!(named["name"], "機房 A");

    // 已知 MAC 連結資產。
    let (status, asset) = send(
        &state,
        Method::POST,
        "/api/v1/assets",
        Some(json!({ "property_no": "PC-099", "description": "未知設備", "location": "機房 A" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "新增資產：{asset}");
    let asset_id = asset["id"].as_i64().expect("資產 id");
    let (status, _) = send(
        &state,
        Method::POST,
        &format!("/api/v1/assets/{asset_id}/interfaces"),
        Some(json!({ "name": "eth0", "mac": "aa:bb:cc:dd:ee:99" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    // 直接植入現況：9.9 較新（已知、有事件）；9.8 較舊（未知、事件已清理）；
    // 10.0.0.2 為一般列（旗標 0）不得出現。
    for (address, seen_at, mac, source, out_of_subnet) in [
        (
            "10.0.9.9",
            "2026-10-06T12:00:00Z",
            Some("aa:bb:cc:dd:ee:99"),
            Some("arp_passive"),
            1,
        ),
        (
            "10.0.9.8",
            "2026-10-05T12:00:00Z",
            Some("aa:bb:cc:dd:ee:88"),
            Some("arp_passive"),
            1,
        ),
        (
            "10.0.0.2",
            "2026-10-06T13:00:00Z",
            Some("aa:bb:cc:00:00:02"),
            Some("arp"),
            0,
        ),
    ] {
        sqlx::query(
            "INSERT INTO ip_presence
                 (subnet_id, address, last_seen_at, last_seen_mac, last_seen_source,
                  last_checked_at, out_of_subnet)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(id)
        .bind(address)
        .bind(seen_at)
        .bind(mac)
        .bind(source)
        .bind("2026-10-06T13:00:00Z")
        .bind(out_of_subnet)
        .execute(&pool)
        .await
        .expect("植入現況列");
    }
    // 9.9 的事件早於最後可見（首見應取事件時間）。
    sqlx::query(
        "INSERT INTO observation_event (subnet_id, address, mac, kind, source, observed_at)
         VALUES (?, '10.0.9.9', 'aa:bb:cc:dd:ee:99', 'first_seen', 'arp_passive',
                 '2026-10-01T08:00:00Z')",
    )
    .bind(id)
    .execute(&pool)
    .await
    .expect("植入事件");

    let json = unmanaged_list(&state).await;
    let items = json["items"].as_array().expect("items 為陣列");
    assert_eq!(items.len(), 2, "只列 out_of_subnet=1：{items:?}");

    // 新到舊：9.9（10/06）在 9.8（10/05）之前。
    assert_eq!(items[0]["address"], "10.0.9.9");
    assert_eq!(items[0]["subnet_id"], id);
    assert_eq!(items[0]["subnet_cidr"], "10.0.0.0/29");
    assert_eq!(items[0]["subnet_name"], "機房 A");
    assert_eq!(items[0]["mac"], "aa:bb:cc:dd:ee:99");
    assert_eq!(items[0]["last_seen_at"], "2026-10-06T12:00:00Z");
    assert_eq!(
        items[0]["first_seen_at"], "2026-10-01T08:00:00Z",
        "首見取最早事件時間"
    );
    assert_eq!(items[0]["source"], "arp_passive");
    assert_eq!(items[0]["known"], true);
    assert_eq!(items[0]["asset"]["id"], asset_id);
    assert_eq!(items[0]["asset"]["description"], "未知設備");
    assert_eq!(items[0]["asset"]["location"], "機房 A");
    assert_eq!(items[0]["asset"]["property_no"], "PC-099");

    // 未知 MAC：known=false、不帶 asset；事件已清理時首見退化為最後可見。
    assert_eq!(items[1]["address"], "10.0.9.8");
    assert_eq!(items[1]["known"], false);
    assert!(
        items[1].get("asset").is_none(),
        "未知 MAC 不帶資產資訊：{:?}",
        items[1]
    );
    assert_eq!(
        items[1]["first_seen_at"], "2026-10-05T12:00:00Z",
        "無事件時首見退化為最後可見"
    );

    // 清空後回空清單。
    sqlx::query("DELETE FROM ip_presence")
        .execute(&pool)
        .await
        .expect("清空現況");
    let json = unmanaged_list(&state).await;
    assert_eq!(json["items"].as_array().map(Vec::len), Some(0));
}

/// 真機唯讀：以系統探測器對本機所在 LAN 被動監聽 ARP 短窗（只收不送；
/// 不改任何設定）。
///
/// 執行：`cargo test --manifest-path backend/Cargo.toml --test observation_passive -- --ignored --nocapture`
#[cfg(target_os = "linux")]
#[tokio::test]
#[ignore = "需要真機網路（本機同 L2）與 CAP_NET_RAW"]
async fn system_prober_passively_observes_live_lan_read_only() {
    // 以 UDP connect 取得對外路由所用的本機位址；不會送出任何封包。
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").expect("綁定 UDP socket");
    socket
        .connect("1.1.1.1:9")
        .expect("建立路由查詢（connect 不送封包）");
    let local = match socket.local_addr().expect("讀取本機位址").ip() {
        std::net::IpAddr::V4(ip) if !ip.is_loopback() => ip,
        other => panic!("找不到對外 IPv4 介面（{other}）；本測試需本機同 L2 的網路"),
    };

    // 無 netmask 可讀時以 /24 近似；僅影響本機判定。
    let network = ipnet::Ipv4Net::new(local, 24).expect("合法 /24").trunc();
    let subnet = Subnet {
        id: 0,
        cidr: network.to_string(),
        name: None,
        note: None,
        gateway: None,
        kea_subnet_id: None,
        observed: true,
        discovery_enabled: false,
        discovery_interval_minutes: None,
        last_discovery_at: None,
        pools: Vec::new(),
        created_at: String::new(),
        updated_at: String::new(),
    };

    let prober = asset_nest::probe::SystemProber::new();
    assert!(
        prober.is_local(&subnet),
        "本機位址必須落在自身 /24 內（{network}）"
    );

    let senders = prober.passive_observe(&subnet, Duration::from_secs(5));
    println!("本機 {local}、網段 {network} → 被動 sender {senders:?}");
    for (address, mac) in &senders {
        assert_ne!(*address, local, "不得回報本機自身位址：{address}");
        assert!(is_mac_like(mac), "MAC 格式不合理：{mac}");
    }
    if senders.is_empty() {
        println!("（本次窗內無 ARP sender；設備閒置或權限不足皆可能）");
    }
}

/// 寬鬆 MAC 檢查：`:` 分隔的 6 組兩位十六進位（小寫正規格式）。
#[cfg(target_os = "linux")]
fn is_mac_like(value: &str) -> bool {
    let groups: Vec<&str> = value.split(':').collect();
    groups.len() == 6
        && groups.iter().all(|group| {
            group.len() == 2
                && group
                    .chars()
                    .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        })
}
