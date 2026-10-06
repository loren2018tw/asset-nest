//! 探索掃描與探索設定整合測試（見票 05、spec §掃描服務／環境設定／HTTP API）。
//!
//! 以注入的 stub 探測邊界驗證全 host 範圍目標、未指派語意與設定驗證；
//! 限速以避免拖慢測試（`with_discovery_rate_pps` 巨大＝單一批次，不進睡眠）。

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

    /// 本檔不測被動監聽；固定回空（被動語意見 `observation_passive.rs`）。
    fn passive_observe(
        &self,
        _subnet: &Subnet,
        _window: std::time::Duration,
    ) -> Vec<(Ipv4Addr, String)> {
        Vec::new()
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

/// 建立附掛 stub 探測邊界的 AppState；速率上限拉大＝單一批次、不睡眠。
fn test_state(pool: &SqlitePool, prober: Arc<StubProber>) -> AppState {
    AppState::new(
        pool.clone(),
        std::env::temp_dir().join("asset-nest-test-no-dist"),
    )
    .with_prober(prober)
    .with_discovery_rate_pps(u32::MAX)
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

/// 開啟網段觀測（快速掃描前提）。
async fn enable_observation(state: &AppState, id: i64) {
    let json = patch_subnet(state, id, json!({ "observed": true })).await;
    assert_eq!(json["observed"], true);
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

/// 發送探索掃描並斷言 400，回傳錯誤 JSON。
async fn discovery_sweep_rejected(state: &AppState, id: i64) -> Value {
    let (status, json) = send(
        state,
        Method::POST,
        &format!("/api/v1/subnets/{id}/sweeps"),
        Some(json!({ "mode": "discovery" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "應拒絕探索掃描：{json}");
    json
}

/// 讀取某網段的 IP 清單並斷言成功。
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

/// 讀取 `observation_event` 的（位址、MAC、種類、來源），依寫入順序。
async fn events(pool: &SqlitePool) -> Vec<(String, String, String, String)> {
    sqlx::query_as(
        "SELECT address, COALESCE(mac, ''), kind, source FROM observation_event ORDER BY id ASC",
    )
    .fetch_all(pool)
    .await
    .expect("讀取觀測事件")
}

/// 讀取某網段 `ip_presence` 的位址（升冪）。
async fn presence_addresses(pool: &SqlitePool, subnet_id: i64) -> Vec<String> {
    sqlx::query_scalar("SELECT address FROM ip_presence WHERE subnet_id = ? ORDER BY address")
        .bind(subnet_id)
        .fetch_all(pool)
        .await
        .expect("讀取現況列")
}

#[tokio::test]
async fn discovery_probes_full_host_range_and_updates_last_discovery_at() {
    let pool = test_pool().await;
    let stub = Arc::new(StubProber::new(&["10.0.0.0/29"]));
    let state = test_state(&pool, stub.clone());

    let id = create_subnet(&state, "10.0.0.0/29").await;
    enable_observation(&state, id).await;
    let enabled = patch_subnet(&state, id, json!({ "discovery_enabled": true })).await;
    assert_eq!(enabled["discovery_enabled"], true);
    assert_eq!(enabled["last_discovery_at"], Value::Null, "尚未探索");

    // 僅未指派的 .5 回應。
    stub.set_responses(&[("10.0.0.5", "AA:BB:CC:00:00:05")]);
    let report = discovery_sweep(&state, id).await;

    assert_eq!(report["mode"], "discovery");
    assert_eq!(report["targets"], 6, "目標＝/29 的全部 host 位址");
    assert_eq!(report["seen"], 1, "seen＝有證據的位址數");
    assert!(report["duration_ms"].is_u64(), "報告須含毫秒計時：{report}");
    let last_discovery = report["last_discovery_at"]
        .as_str()
        .expect("探索報告須含 last_discovery_at");
    assert!(
        last_discovery.ends_with('Z'),
        "UTC 時間戳：{last_discovery}"
    );

    assert_eq!(
        stub.calls(),
        vec![vec![
            "10.0.0.1".parse::<Ipv4Addr>().expect("一"),
            "10.0.0.2".parse::<Ipv4Addr>().expect("二"),
            "10.0.0.3".parse::<Ipv4Addr>().expect("三"),
            "10.0.0.4".parse::<Ipv4Addr>().expect("四"),
            "10.0.0.5".parse::<Ipv4Addr>().expect("五"),
            "10.0.0.6".parse::<Ipv4Addr>().expect("六"),
        ]],
        "探索一次探測全 host 範圍（依數值升冪；巨大速率＝單一批次）"
    );

    // 詳情即時反映 last_discovery_at。
    let (status, detail) = send(&state, Method::GET, &format!("/api/v1/subnets/{id}"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail["last_discovery_at"], last_discovery);
    assert_eq!(detail["discovery_enabled"], true);

    // 未指派被看見：建現況列＋first_seen。
    let page = list_ips(&state, id).await;
    let seen = row(&page, "10.0.0.5");
    assert_eq!(seen["last_seen_mac"], "aa:bb:cc:00:00:05", "MAC 正規化");
    assert_eq!(seen["last_seen_source"], "arp");
    assert!(
        seen["last_seen_at"].is_string(),
        "被看見者須有 last_seen_at：{seen}"
    );
    assert_eq!(
        events(&pool).await,
        vec![(
            "10.0.0.5".to_string(),
            "aa:bb:cc:00:00:05".to_string(),
            "first_seen".to_string(),
            "arp".to_string(),
        )]
    );
}

#[tokio::test]
async fn discovery_checks_assigned_unseen_and_only_rows_unknown_when_seen() {
    let pool = test_pool().await;
    let stub = Arc::new(StubProber::new(&["10.0.0.0/29"]));
    let state = test_state(&pool, stub.clone());

    let id = create_subnet(&state, "10.0.0.0/29").await;
    enable_observation(&state, id).await;
    patch_subnet(&state, id, json!({ "discovery_enabled": true })).await;

    // 指派 .1（static）並建立資產。
    let (status, asset) = send(
        &state,
        Method::POST,
        "/api/v1/assets",
        Some(json!({ "description": "主機", "location": "機房" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "新增資產：{asset}");
    let asset_id = asset["id"].as_i64().expect("資產 id");
    let (status, interface) = send(
        &state,
        Method::POST,
        &format!("/api/v1/assets/{asset_id}/interfaces"),
        Some(json!({ "name": "eth0", "mac": "aa:bb:cc:dd:ee:01" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "新增介面：{interface}");
    let interface_id = interface["id"].as_i64().expect("介面 id");
    let (status, assigned) = send(
        &state,
        Method::PUT,
        &format!("/api/v1/subnets/{id}/ips/10.0.0.1/assignment"),
        Some(json!({ "interface_id": interface_id, "purpose": "static" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "指派：{assigned}");

    // 第一輪：指派 .1 未回應；未指派的 .3 有回應。
    stub.set_responses(&[("10.0.0.3", "aa:bb:cc:00:00:03")]);
    let report = discovery_sweep(&state, id).await;
    assert_eq!(report["targets"], 6);
    assert_eq!(report["seen"], 1);

    // 指派未回應：僅 last_checked_at；未指派未回應：完全不建列。
    assert_eq!(
        presence_addresses(&pool, id).await,
        ["10.0.0.1".to_string(), "10.0.0.3".to_string()],
        "只有指派位址與被看見的未指派位址有現況列"
    );
    let page = list_ips(&state, id).await;
    let unseen_assigned = row(&page, "10.0.0.1");
    assert!(
        unseen_assigned["last_checked_at"].is_string(),
        "指派未回應仍更新 last_checked_at：{unseen_assigned}"
    );
    assert!(unseen_assigned["last_seen_at"].is_null());
    assert!(unseen_assigned["last_seen_mac"].is_null());

    // 未指派被看見：現況列＋first_seen。
    let unknown = row(&page, "10.0.0.3");
    assert_eq!(unknown["last_seen_mac"], "aa:bb:cc:00:00:03");
    assert_eq!(unknown["last_seen_source"], "arp");
    assert_eq!(
        events(&pool).await,
        vec![(
            "10.0.0.3".to_string(),
            "aa:bb:cc:00:00:03".to_string(),
            "first_seen".to_string(),
            "arp".to_string(),
        )]
    );

    // 第二輪：.1 出現（first_seen）、.3 換 MAC（mac_changed）。
    stub.set_responses(&[
        ("10.0.0.1", "aa:bb:cc:dd:ee:01"),
        ("10.0.0.3", "aa:bb:cc:00:00:33"),
    ]);
    let report = discovery_sweep(&state, id).await;
    assert_eq!(report["seen"], 2, "兩者皆有證據");

    let page = list_ips(&state, id).await;
    assert_eq!(row(&page, "10.0.0.1")["last_seen_mac"], "aa:bb:cc:dd:ee:01");
    assert_eq!(row(&page, "10.0.0.3")["last_seen_mac"], "aa:bb:cc:00:00:33");
    assert_eq!(
        events(&pool).await,
        vec![
            (
                "10.0.0.3".to_string(),
                "aa:bb:cc:00:00:03".to_string(),
                "first_seen".to_string(),
                "arp".to_string(),
            ),
            (
                "10.0.0.1".to_string(),
                "aa:bb:cc:dd:ee:01".to_string(),
                "first_seen".to_string(),
                "arp".to_string(),
            ),
            (
                "10.0.0.3".to_string(),
                "aa:bb:cc:00:00:33".to_string(),
                "mac_changed".to_string(),
                "arp".to_string(),
            ),
        ]
    );
}

#[tokio::test]
async fn discovery_sweep_rejected_without_discovery_or_local_scope() {
    let pool = test_pool().await;
    let stub = Arc::new(StubProber::new(&["10.0.0.0/29"]));
    let state = test_state(&pool, stub.clone());

    // 已開觀測但未開探索 → 400；不觸發任何探測。
    let id = create_subnet(&state, "10.0.0.0/29").await;
    enable_observation(&state, id).await;
    let body = discovery_sweep_rejected(&state, id).await;
    assert_eq!(body["error"], "validation_error");
    assert_eq!(body["details"]["field"], "discovery_enabled");
    assert!(
        body["message"]
            .as_str()
            .is_some_and(|message| message.contains("探索")),
        "訊息須說明未開探索：{body}"
    );
    assert!(stub.calls().is_empty(), "被拒時不得探測");

    // 已開觀測＋探索但非同 L2 → 400。
    let remote = create_subnet(&state, "10.0.9.0/29").await;
    enable_observation(&state, remote).await;
    patch_subnet(&state, remote, json!({ "discovery_enabled": true })).await;
    let body = discovery_sweep_rejected(&state, remote).await;
    assert!(
        body["message"]
            .as_str()
            .is_some_and(|message| message.contains("L2")),
        "非同 L2 訊息：{body}"
    );

    // v6 → 400（結構上不支援）。
    let v6 = create_subnet(&state, "fd43::/64").await;
    let body = discovery_sweep_rejected(&state, v6).await;
    assert!(
        body["message"]
            .as_str()
            .is_some_and(|message| message.contains("IPv6")),
        "v6 訊息：{body}"
    );

    // 不存在 → 404。
    let (status, _) = send(
        &state,
        Method::POST,
        "/api/v1/subnets/999/sweeps",
        Some(json!({ "mode": "discovery" })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn discovery_patch_validation_and_roundtrip() {
    let pool = test_pool().await;
    let stub = Arc::new(StubProber::new(&[]));
    let state = test_state(&pool, stub);

    // 未開觀測：開探索 → 400。
    let id = create_subnet(&state, "10.20.1.0/24").await;
    let (status, body) = send(
        &state,
        Method::PATCH,
        &format!("/api/v1/subnets/{id}"),
        Some(json!({ "discovery_enabled": true })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["details"]["field"], "discovery_enabled");
    assert!(
        body["message"]
            .as_str()
            .is_some_and(|message| message.contains("觀測")),
        "訊息須說明需先開觀測：{body}"
    );

    // 先開觀測，再開探索並帶間隔；詳情回傳。
    enable_observation(&state, id).await;
    let enabled = patch_subnet(
        &state,
        id,
        json!({ "discovery_enabled": true, "discovery_interval_minutes": 120 }),
    )
    .await;
    assert_eq!(enabled["discovery_enabled"], true);
    assert_eq!(enabled["discovery_interval_minutes"], 120);
    assert_eq!(enabled["last_discovery_at"], Value::Null);

    let (status, detail) = send(&state, Method::GET, &format!("/api/v1/subnets/{id}"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail["discovery_enabled"], true);
    assert_eq!(detail["discovery_interval_minutes"], 120);

    // 摘要維持既有欄位（不含探索欄位）。
    let (status, page) = send(&state, Method::GET, "/api/v1/subnets", None).await;
    assert_eq!(status, StatusCode::OK);
    let item = &page["items"][0];
    assert!(item.get("discovery_enabled").is_none(), "摘要不含探索欄位");
    assert!(item.get("last_discovery_at").is_none());

    // 顯式 null＝清除（回全站預設）。
    let cleared = patch_subnet(&state, id, json!({ "discovery_interval_minutes": null })).await;
    assert_eq!(cleared["discovery_interval_minutes"], Value::Null);

    // 間隔須為正整數。
    for invalid in [json!(0), json!(-3)] {
        let (status, body) = send(
            &state,
            Method::PATCH,
            &format!("/api/v1/subnets/{id}"),
            Some(json!({ "discovery_interval_minutes": invalid })),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "應拒絕：{invalid}");
        assert_eq!(body["details"]["field"], "discovery_interval_minutes");
    }
    for invalid in [json!(1.5), json!("60")] {
        let (status, _) = send(
            &state,
            Method::PATCH,
            &format!("/api/v1/subnets/{id}"),
            Some(json!({ "discovery_interval_minutes": invalid })),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "非整數應拒絕：{invalid}");
    }

    // 關閉探索後，關閉觀測為合法；反之（探索仍開）維持 400。
    patch_subnet(&state, id, json!({ "discovery_enabled": false })).await;
    let off = patch_subnet(&state, id, json!({ "observed": false })).await;
    assert_eq!(off["observed"], false);

    enable_observation(&state, id).await;
    patch_subnet(&state, id, json!({ "discovery_enabled": true })).await;
    let (status, body) = send(
        &state,
        Method::PATCH,
        &format!("/api/v1/subnets/{id}"),
        Some(json!({ "observed": false })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "探索仍開時不得關閉觀測");
    assert_eq!(body["details"]["field"], "discovery_enabled");

    // v6 不可開探索（即使從未開觀測）。
    let v6 = create_subnet(&state, "fd42::/64").await;
    let (status, body) = send(
        &state,
        Method::PATCH,
        &format!("/api/v1/subnets/{v6}"),
        Some(json!({ "discovery_enabled": true })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body["message"]
            .as_str()
            .is_some_and(|message| message.contains("IPv6")),
        "v6 訊息：{body}"
    );
}
