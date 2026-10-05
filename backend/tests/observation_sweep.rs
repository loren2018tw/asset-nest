//! 快速掃描與「最後可見」欄整合測試（見票 02、spec §探測邊界／掃描服務／HTTP API）。
//!
//! 以注入的 stub 探測邊界驗證探測目標集合與觀測寫入；真機 ARP 測試為
//! `#[ignore]`，預設不執行（執行方式見檔尾）。

use std::net::IpAddr;
use std::net::Ipv4Addr;
use std::sync::{Arc, Mutex};

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

/// 可腳本化的探測邊界 stub：記錄每次探測的目標，回傳腳本中的回應。
struct StubProber {
    local_cidrs: Vec<String>,
    /// 腳本回應（位址、MAC）；只回傳命中該次目標集合者。
    responses: Mutex<Vec<(Ipv4Addr, String)>>,
    /// 每次 `probe` 收到的目標集合（依呼叫順序）。
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

    /// 設定回應腳本（取代先前內容）。
    fn set_responses(&self, responses: &[(&str, &str)]) {
        *self.responses.lock().expect("stub 鎖") = responses
            .iter()
            .map(|(address, mac)| (address.parse().expect("合法位址"), mac.to_string()))
            .collect();
    }

    /// 每次 `probe` 收到的目標集合。
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

/// 建立附掛 stub 探測邊界的 AppState。
fn test_state(pool: &SqlitePool, prober: Arc<StubProber>) -> AppState {
    AppState::new(
        pool.clone(),
        std::env::temp_dir().join("asset-nest-test-no-dist"),
    )
    .with_prober(prober)
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
    assert_eq!(json["observed"], true);
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

/// 讀取某網段的 IP 清單（`query` 含開頭 `?` 或空字串）並斷言成功。
async fn list_ips(state: &AppState, id: i64, query: &str) -> Value {
    let (status, json) = send(
        state,
        Method::GET,
        &format!("/api/v1/subnets/{id}/ips{query}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "讀取 IP 清單應成功：{json}");
    json
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

/// 取出回應列中的位址字串。
fn addresses(page: &Value) -> Vec<&str> {
    page["items"]
        .as_array()
        .expect("items 為陣列")
        .iter()
        .map(|item| item["address"].as_str().expect("address 為字串"))
        .collect()
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

/// 讀取 `observation_event` 的（位址、MAC、種類、來源），依寫入順序。
async fn events(pool: &SqlitePool) -> Vec<(String, String, String, String)> {
    sqlx::query_as(
        "SELECT address, COALESCE(mac, ''), kind, source FROM observation_event ORDER BY id ASC",
    )
    .fetch_all(pool)
    .await
    .expect("讀取觀測事件")
}

#[tokio::test]
async fn quick_sweep_probes_assigned_addresses_and_records_presence() {
    let pool = test_pool().await;
    let stub = Arc::new(StubProber::new(&["10.0.0.0/29"]));
    let state = test_state(&pool, stub.clone());

    let subnet = create_subnet(&state, json!({ "cidr": "10.0.0.0/29" })).await;
    let id = subnet["id"].as_i64().expect("回應含 id");
    enable_observation(&state, id).await;

    let first = create_asset(&state, "主機一").await;
    let first_interface = create_interface(
        &state,
        first,
        json!({ "name": "eth0", "mac": "aa:bb:cc:dd:ee:01" }),
    )
    .await;
    let second = create_asset(&state, "主機二").await;
    let second_interface = create_interface(
        &state,
        second,
        json!({ "name": "eth0", "mac": "aa:bb:cc:dd:ee:02" }),
    )
    .await;
    assign_ip(
        &state,
        id,
        "10.0.0.1",
        json!({ "interface_id": first_interface, "purpose": "static" }),
    )
    .await;
    assign_ip(
        &state,
        id,
        "10.0.0.2",
        json!({ "interface_id": second_interface, "purpose": "static" }),
    )
    .await;

    // 僅 .1 回應；MAC 以大寫腳本，寫入時須正規化為小寫冒號格式。
    stub.set_responses(&[("10.0.0.1", "AA:BB:CC:00:00:01")]);

    let report = quick_sweep(&state, id).await;
    assert_eq!(report["mode"], "quick");
    assert_eq!(report["targets"], 2, "目標數＝已指派位址數");
    assert_eq!(report["seen"], 1, "seen＝有回應的位址數");
    assert!(report["duration_ms"].is_u64(), "報告須含毫秒計時：{report}");

    assert_eq!(
        stub.calls(),
        vec![vec![
            "10.0.0.1".parse::<Ipv4Addr>().expect("位址一"),
            "10.0.0.2".parse::<Ipv4Addr>().expect("位址二"),
        ]],
        "快速掃描僅探測該網段已指派位址（依數值升冪）"
    );

    // 回應者：last_seen 三欄＋last_checked；來源 arp、MAC 正規化。
    let page = list_ips(&state, id, "").await;
    let seen = row(&page, "10.0.0.1");
    assert_eq!(seen["observed"], true, "已開觀測且同 L2：有效涵蓋");
    assert!(
        seen["last_seen_at"]
            .as_str()
            .is_some_and(|value| value.ends_with('Z')),
        "last_seen_at 須為 UTC 時間戳：{seen}"
    );
    assert_eq!(
        seen["last_seen_mac"], "aa:bb:cc:00:00:01",
        "MAC 正規化為小寫"
    );
    assert_eq!(seen["last_seen_source"], "arp");
    assert!(
        seen["last_checked_at"].is_string(),
        "回應者亦更新 last_checked_at：{seen}"
    );

    // 未回應者：僅更新 last_checked_at，last_seen 維持 NULL。
    let unseen = row(&page, "10.0.0.2");
    assert_eq!(unseen["observed"], true);
    assert!(unseen["last_seen_at"].is_null(), "未回應不得寫 last_seen");
    assert!(unseen["last_seen_mac"].is_null());
    assert!(unseen["last_seen_source"].is_null());
    assert!(
        unseen["last_checked_at"].is_string(),
        "未回應者仍更新 last_checked_at：{unseen}"
    );

    // 首次看到寫 first_seen（append-only）。
    assert_eq!(
        events(&pool).await,
        vec![(
            "10.0.0.1".to_string(),
            "aa:bb:cc:00:00:01".to_string(),
            "first_seen".to_string(),
            "arp".to_string(),
        )]
    );
}

#[tokio::test]
async fn rescan_same_mac_writes_no_event_and_changed_mac_writes_mac_changed() {
    let pool = test_pool().await;
    let stub = Arc::new(StubProber::new(&["10.0.0.0/29"]));
    let state = test_state(&pool, stub.clone());

    let subnet = create_subnet(&state, json!({ "cidr": "10.0.0.0/29" })).await;
    let id = subnet["id"].as_i64().expect("回應含 id");
    enable_observation(&state, id).await;

    let asset = create_asset(&state, "主機").await;
    let interface = create_interface(
        &state,
        asset,
        json!({ "name": "eth0", "mac": "aa:bb:cc:dd:ee:01" }),
    )
    .await;
    assign_ip(
        &state,
        id,
        "10.0.0.1",
        json!({ "interface_id": interface, "purpose": "static" }),
    )
    .await;

    // 第一次：first_seen。
    stub.set_responses(&[("10.0.0.1", "aa:bb:cc:dd:ee:01")]);
    quick_sweep(&state, id).await;

    // 重掃同 MAC：不重複寫事件，last_seen 持續更新。
    quick_sweep(&state, id).await;
    let after_same = events(&pool).await;
    assert_eq!(
        after_same.len(),
        1,
        "同 MAC 重掃不得重複寫事件：{after_same:?}"
    );
    assert_eq!(after_same[0].2, "first_seen");

    // 換 MAC：mac_changed，舊事件保留（append-only）。
    stub.set_responses(&[("10.0.0.1", "AA:BB:CC:DD:EE:99")]);
    quick_sweep(&state, id).await;
    let after_change = events(&pool).await;
    assert_eq!(
        after_change.len(),
        2,
        "換 MAC 寫入第二筆事件：{after_change:?}"
    );
    assert_eq!(after_change[0].2, "first_seen");
    assert_eq!(after_change[1].0, "10.0.0.1");
    assert_eq!(after_change[1].1, "aa:bb:cc:dd:ee:99", "事件 MAC 正規化");
    assert_eq!(after_change[1].2, "mac_changed");
    assert_eq!(after_change[1].3, "arp");

    let page = list_ips(&state, id, "").await;
    assert_eq!(
        row(&page, "10.0.0.1")["last_seen_mac"],
        "aa:bb:cc:dd:ee:99",
        "現況取最近者"
    );
}

#[tokio::test]
async fn sweep_rejects_unknown_mode_v6_unobserved_and_non_local() {
    let pool = test_pool().await;
    let stub = Arc::new(StubProber::new(&["10.0.0.0/29", "10.0.1.0/29"]));
    let state = test_state(&pool, stub);

    let subnet = create_subnet(&state, json!({ "cidr": "10.0.0.0/29" })).await;
    let id = subnet["id"].as_i64().expect("回應含 id");

    // 未知模式與未實作的 discovery → 400（mode 先於網段前提驗證）。
    for (body, fragment) in [
        (json!({ "mode": "nope" }), "模式"),
        (json!({ "mode": "discovery" }), "探索"),
        (json!({}), "mode"),
    ] {
        let (status, json) = send(
            &state,
            Method::POST,
            &format!("/api/v1/subnets/{id}/sweeps"),
            Some(body),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "應拒絕：{json}");
        assert_eq!(json["error"], "validation_error");
        assert!(
            json["message"]
                .as_str()
                .is_some_and(|message| message.contains(fragment)),
            "訊息須說明原因（{fragment}）：{json}"
        );
    }

    // v6 → 400（觀測結構上不支援）。
    let v6 = create_subnet(&state, json!({ "cidr": "fd42::/64" })).await;
    let v6_id = v6["id"].as_i64().expect("回應含 id");
    let (status, json) = send(
        &state,
        Method::POST,
        &format!("/api/v1/subnets/{v6_id}/sweeps"),
        Some(json!({ "mode": "quick" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        json["message"]
            .as_str()
            .is_some_and(|message| message.contains("IPv6")),
        "v6 訊息：{json}"
    );

    // 未開觀測 → 400。
    let (status, json) = send(
        &state,
        Method::POST,
        &format!("/api/v1/subnets/{id}/sweeps"),
        Some(json!({ "mode": "quick" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        json["message"]
            .as_str()
            .is_some_and(|message| message.contains("未開啟觀測")),
        "未開觀測訊息：{json}"
    );

    // 已開觀測但非同 L2 → 400；清單列的 observed 亦為 false（有效涵蓋）。
    let remote = create_subnet(&state, json!({ "cidr": "10.0.2.0/29" })).await;
    let remote_id = remote["id"].as_i64().expect("回應含 id");
    enable_observation(&state, remote_id).await;
    let (status, json) = send(
        &state,
        Method::POST,
        &format!("/api/v1/subnets/{remote_id}/sweeps"),
        Some(json!({ "mode": "quick" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        json["message"]
            .as_str()
            .is_some_and(|message| message.contains("L2")),
        "非同 L2 訊息：{json}"
    );
    let page = list_ips(&state, remote_id, "").await;
    assert_eq!(
        page["items"][0]["observed"], false,
        "非同 L2 時有效涵蓋為未觀測"
    );

    // 不存在的網段 → 404。
    let (status, _) = send(
        &state,
        Method::POST,
        "/api/v1/subnets/999/sweeps",
        Some(json!({ "mode": "quick" })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn ip_list_exposes_last_seen_fields_and_nulls_last_sort() {
    let pool = test_pool().await;
    let stub = Arc::new(StubProber::new(&["10.0.0.0/29", "10.0.1.0/29"]));
    let state = test_state(&pool, stub);

    let subnet = create_subnet(&state, json!({ "cidr": "10.0.0.0/29" })).await;
    let id = subnet["id"].as_i64().expect("回應含 id");
    enable_observation(&state, id).await;

    // 直接植入現況列：.1 較新（arp）、.2 較舊（kea_lease）、.3 從未上線。
    for (address, seen_at, mac, source) in [
        (
            "10.0.0.1",
            Some("2026-10-06T10:00:00Z"),
            Some("aa:bb:cc:dd:ee:01"),
            Some("arp"),
        ),
        (
            "10.0.0.2",
            Some("2026-10-05T10:00:00Z"),
            Some("aa:bb:cc:dd:ee:02"),
            Some("kea_lease"),
        ),
        ("10.0.0.3", None, None, None),
    ] {
        sqlx::query(
            "INSERT INTO ip_presence
                 (subnet_id, address, last_seen_at, last_seen_mac, last_seen_source, last_checked_at)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(id)
        .bind(address)
        .bind(seen_at)
        .bind(mac)
        .bind(source)
        .bind("2026-10-06T10:00:00Z")
        .execute(&pool)
        .await
        .expect("植入現況列");
    }

    // 欄位：三態所需的資料齊全，未指派列從未上線。
    let page = list_ips(&state, id, "").await;
    let seen = row(&page, "10.0.0.1");
    assert_eq!(seen["observed"], true);
    assert_eq!(seen["last_seen_at"], "2026-10-06T10:00:00Z");
    assert_eq!(seen["last_seen_mac"], "aa:bb:cc:dd:ee:01");
    assert_eq!(seen["last_seen_source"], "arp");
    assert_eq!(seen["last_checked_at"], "2026-10-06T10:00:00Z");

    let never = row(&page, "10.0.0.3");
    assert_eq!(never["observed"], true);
    assert!(never["last_seen_at"].is_null());
    assert_eq!(never["last_checked_at"], "2026-10-06T10:00:00Z");

    let untouched = row(&page, "10.0.0.4");
    assert!(untouched["last_seen_at"].is_null());
    assert!(untouched["last_checked_at"].is_null());

    // 排序：seen 依時間；NULL（未上線＋未指派）固定最後，不分升降冪。
    let asc = list_ips(&state, id, "?sort=last_seen&dir=asc").await;
    assert_eq!(
        addresses(&asc),
        [
            "10.0.0.2", "10.0.0.1", "10.0.0.3", "10.0.0.4", "10.0.0.5", "10.0.0.6"
        ],
        "asc：時間舊→新，NULL 依位址排最後"
    );

    let desc = list_ips(&state, id, "?sort=last_seen&dir=desc").await;
    assert_eq!(
        addresses(&desc),
        [
            "10.0.0.1", "10.0.0.2", "10.0.0.3", "10.0.0.4", "10.0.0.5", "10.0.0.6"
        ],
        "desc：時間新→舊，NULL 仍排最後"
    );

    // 未開觀測的網段：有效涵蓋恆為 false、不帶現況。
    let off = create_subnet(&state, json!({ "cidr": "10.0.1.0/29" })).await;
    let off_id = off["id"].as_i64().expect("回應含 id");
    let off_page = list_ips(&state, off_id, "").await;
    assert_eq!(off_page["items"][0]["observed"], false);
    assert!(off_page["items"][0]["last_seen_at"].is_null());
}

/// 真機唯讀：以系統探測器對本機所在 LAN 發 ARP 請求（僅送 ARP；不改任何設定）。
///
/// 執行：`cargo test --manifest-path backend/Cargo.toml --test observation_sweep -- --ignored --nocapture`
#[cfg(target_os = "linux")]
#[tokio::test]
#[ignore = "需要真機網路（本機同 L2）"]
async fn system_prober_probes_live_lan_read_only() {
    // 以 UDP connect 取得對外路由所用的本機位址；不會送出任何封包。
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").expect("綁定 UDP socket");
    socket
        .connect("1.1.1.1:9")
        .expect("建立路由查詢（connect 不送封包）");
    let local = match socket.local_addr().expect("讀取本機位址").ip() {
        IpAddr::V4(ip) if !ip.is_loopback() => ip,
        other => panic!("找不到對外 IPv4 介面（{other}）；本測試需本機同 L2 的網路"),
    };

    // 無 netmask 可讀時以 /24 近似；僅影響 is_local 判定與目標集合。
    let network = ipnet::Ipv4Net::new(local, 24).expect("合法 /24").trunc();
    let subnet = Subnet {
        id: 0,
        cidr: network.to_string(),
        name: None,
        note: None,
        gateway: None,
        kea_subnet_id: None,
        observed: true,
        pools: Vec::new(),
        created_at: String::new(),
        updated_at: String::new(),
    };

    let prober = asset_nest::probe::SystemProber::new();
    assert!(
        prober.is_local(&subnet),
        "本機位址必須落在自身 /24 內（{network}）"
    );

    // 目標：預設閘道（由 /proc/net/route 讀取）；讀不到則用第一個 host。
    let gateway = default_gateway();
    let targets: Vec<Ipv4Addr> = gateway.into_iter().chain(network.hosts().take(1)).collect();
    let responses = prober.probe(&subnet, &targets);

    println!("本機 {local}、網段 {network}、目標 {targets:?} → ARP 回應 {responses:?}");
    for (ip, mac) in &responses {
        assert!(targets.contains(ip), "回應位址須在目標集合內：{ip}");
        assert!(is_mac_like(mac), "MAC 格式不合理：{mac}");
    }
    if responses.is_empty() {
        println!("（本次無 ARP 回應；可能目標裝置不存在或探測權限不足）");
    }
}

/// 由 `/proc/net/route` 讀取預設閘道（無預設路由或非 Linux 回 `None`）。
#[cfg(target_os = "linux")]
fn default_gateway() -> Option<Ipv4Addr> {
    let table = std::fs::read_to_string("/proc/net/route").ok()?;
    for line in table.lines().skip(1) {
        let words: Vec<&str> = line.split_whitespace().collect();
        if words.len() < 3 || words[1] != "00000000" {
            continue;
        }
        let raw = u32::from_str_radix(words[2], 16).ok()?;
        let address = Ipv4Addr::from(raw.swap_bytes());
        if !address.is_unspecified() {
            return Some(address);
        }
    }
    None
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
