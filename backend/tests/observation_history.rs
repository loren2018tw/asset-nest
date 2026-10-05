//! 觀測歷史端點整合測試（見票 06、spec §讀取端／§HTTP API）。
//!
//! 以注入的 stub 探測邊界（比照 `observation_sweep.rs`）驗證：
//! - IP 歷史：現況＋事件（新到舊）＋用過的 MAC 彙總（首見／最後可見／來源／已知）。
//! - MAC 歷史：用過的位址每位址首見／最後可見／來源，已知 MAC 連資產。
//! - CSV 匯出：BOM、欄位、事件順序與檔名。
//!
//! 歷史以直接 SQL 植入固定時間戳，讓排序與彙總規則可精確斷言；掃描產生的
//! 事件語意已於 `observation_sweep.rs` 覆蓋。

use std::net::Ipv4Addr;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{HeaderMap, Method, Request, StatusCode, header};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use sqlx::SqlitePool;
use sqlx::sqlite::SqlitePoolOptions;
use tower::ServiceExt;

use asset_nest::probe::Prober;
use asset_nest::subnets::Subnet;
use asset_nest::{AppState, app};

/// 最小 stub 探測邊界：只回答本機同 L2 判定（歷史讀取端不探測）。
struct StubProber {
    local_cidrs: Vec<String>,
}

impl StubProber {
    fn new(local_cidrs: &[&str]) -> Self {
        Self {
            local_cidrs: local_cidrs.iter().map(|cidr| cidr.to_string()).collect(),
        }
    }
}

impl Prober for StubProber {
    fn is_local(&self, subnet: &Subnet) -> bool {
        self.local_cidrs.iter().any(|cidr| cidr == &subnet.cidr)
    }

    fn probe(&self, _subnet: &Subnet, _targets: &[Ipv4Addr]) -> Vec<(Ipv4Addr, String)> {
        Vec::new()
    }
}

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

/// 建立附掛 stub 探測邊界的 AppState。
fn test_state(pool: &SqlitePool) -> AppState {
    AppState::new(
        pool.clone(),
        std::env::temp_dir().join("asset-nest-test-no-dist"),
    )
    .with_prober(Arc::new(StubProber::new(&["10.0.0.0/29", "10.0.1.0/29"])))
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

/// 發送請求並回傳狀態、標頭與原始位元組（CSV 匯出用）。
async fn send_bytes(state: &AppState, uri: &str) -> (StatusCode, HeaderMap, Vec<u8>) {
    let request = Request::builder()
        .method(Method::GET)
        .uri(uri)
        .body(Body::empty())
        .expect("建立請求");

    let response = app(state.clone()).oneshot(request).await.expect("執行請求");

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
async fn create_asset(state: &AppState, body: Value) -> i64 {
    let (status, json) = send(state, Method::POST, "/api/v1/assets", Some(body)).await;
    assert_eq!(status, StatusCode::CREATED, "新增資產應成功：{json}");
    json["id"].as_i64().expect("回應含 id")
}

/// 對資產新增介面並斷言成功，回傳 id。
async fn create_interface(state: &AppState, asset_id: i64, mac: &str) -> i64 {
    let (status, json) = send(
        state,
        Method::POST,
        &format!("/api/v1/assets/{asset_id}/interfaces"),
        Some(json!({ "name": "eth0", "mac": mac })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "新增介面應成功：{json}");
    json["id"].as_i64().expect("回應含 id")
}

/// 植入一筆觀測事件，回傳列 id。
async fn insert_event(
    pool: &SqlitePool,
    subnet_id: i64,
    address: &str,
    mac: &str,
    kind: &str,
    source: &str,
    observed_at: &str,
) -> i64 {
    sqlx::query(
        "INSERT INTO observation_event (subnet_id, address, mac, kind, source, observed_at)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(subnet_id)
    .bind(address)
    .bind(mac)
    .bind(kind)
    .bind(source)
    .bind(observed_at)
    .execute(pool)
    .await
    .expect("植入觀測事件")
    .last_insert_rowid()
}

/// 植入一筆觀測現況。
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

/// 讀取 IP 歷史端點。
async fn ip_history(state: &AppState, subnet_id: i64, address: &str) -> (StatusCode, Value) {
    send(
        state,
        Method::GET,
        &format!("/api/v1/subnets/{subnet_id}/ips/{address}/observations"),
        None,
    )
    .await
}

/// 讀取 MAC 歷史端點。
async fn mac_history(state: &AppState, mac: &str) -> (StatusCode, Value) {
    send(
        state,
        Method::GET,
        &format!("/api/v1/observations/mac/{mac}"),
        None,
    )
    .await
}

#[tokio::test]
async fn ip_history_returns_presence_events_and_used_macs() {
    let pool = test_pool().await;
    let state = test_state(&pool);
    let id = create_subnet(&state, "10.0.0.0/29").await;
    enable_observation(&state, id).await;

    // 已知 MAC 連結的資產（.2 為目前現況 MAC）。
    let asset = create_asset(
        &state,
        json!({ "property_no": "PC-002", "description": "主機二", "location": "機房 B" }),
    )
    .await;
    create_interface(&state, asset, "aa:bb:cc:dd:ee:02").await;

    // 現況取 MAC 02（arp、較新）；歷史：01 首見 → 02 變更（kea_lease）。
    insert_presence(
        &pool,
        id,
        "10.0.0.1",
        Some("2026-10-06T12:00:00Z"),
        Some("aa:bb:cc:dd:ee:02"),
        Some("arp"),
        Some("2026-10-06T12:05:00Z"),
    )
    .await;
    let first = insert_event(
        &pool,
        id,
        "10.0.0.1",
        "aa:bb:cc:dd:ee:01",
        "first_seen",
        "arp",
        "2026-10-01T10:00:00Z",
    )
    .await;
    let changed = insert_event(
        &pool,
        id,
        "10.0.0.1",
        "aa:bb:cc:dd:ee:02",
        "mac_changed",
        "kea_lease",
        "2026-10-05T09:00:00Z",
    )
    .await;

    let (status, json) = ip_history(&state, id, "10.0.0.1").await;
    assert_eq!(status, StatusCode::OK, "讀取 IP 歷史應成功：{json}");

    assert_eq!(json["observed"], true, "已開觀測且同 L2：有效涵蓋");
    assert_eq!(json["presence"]["last_seen_at"], "2026-10-06T12:00:00Z");
    assert_eq!(json["presence"]["last_seen_mac"], "aa:bb:cc:dd:ee:02");
    assert_eq!(json["presence"]["last_seen_source"], "arp");
    assert_eq!(json["presence"]["last_checked_at"], "2026-10-06T12:05:00Z");

    // 事件新到舊：mac_changed（id 較大、時間較新）在前。
    let events = json["events"].as_array().expect("events 為陣列");
    assert_eq!(events.len(), 2);
    assert_eq!(events[0]["id"], changed);
    assert_eq!(events[0]["mac"], "aa:bb:cc:dd:ee:02");
    assert_eq!(events[0]["kind"], "mac_changed");
    assert_eq!(events[0]["source"], "kea_lease");
    assert_eq!(events[0]["observed_at"], "2026-10-05T09:00:00Z");
    assert_eq!(events[1]["id"], first);
    assert_eq!(events[1]["kind"], "first_seen");
    assert_eq!(events[1]["observed_at"], "2026-10-01T10:00:00Z");

    // 用過的 MAC：新到舊；現況 MAC 的「最後可見」以現況為準（含來源）。
    let macs = json["macs"].as_array().expect("macs 為陣列");
    assert_eq!(macs.len(), 2);
    assert_eq!(macs[0]["mac"], "aa:bb:cc:dd:ee:02");
    assert_eq!(macs[0]["first_seen_at"], "2026-10-05T09:00:00Z");
    assert_eq!(
        macs[0]["last_seen_at"], "2026-10-06T12:00:00Z",
        "現況較新時以現況時間為最後可見"
    );
    assert_eq!(macs[0]["source"], "arp", "最後可見來源取最新訊號（現況）");
    assert_eq!(macs[0]["known"], true);
    assert_eq!(macs[0]["asset"]["id"], asset);
    assert_eq!(macs[0]["asset"]["description"], "主機二");
    assert_eq!(macs[0]["asset"]["location"], "機房 B");
    assert_eq!(macs[0]["asset"]["property_no"], "PC-002");

    assert_eq!(macs[1]["mac"], "aa:bb:cc:dd:ee:01");
    assert_eq!(macs[1]["first_seen_at"], "2026-10-01T10:00:00Z");
    assert_eq!(macs[1]["last_seen_at"], "2026-10-01T10:00:00Z");
    assert_eq!(macs[1]["source"], "arp");
    assert_eq!(macs[1]["known"], false, "不在任何 Interface 的 MAC 為未知");
    assert!(
        macs[1].get("asset").is_none(),
        "未知 MAC 不帶資產資訊：{macs:?}"
    );
}

#[tokio::test]
async fn ip_history_empty_and_unobserved_subnets() {
    let pool = test_pool().await;
    let state = test_state(&pool);

    // 未開觀測：observed=false、空歷史。
    let off = create_subnet(&state, "10.0.0.0/29").await;
    let (status, json) = ip_history(&state, off, "10.0.0.1").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["observed"], false, "未開觀測：未觀測");
    assert!(json["presence"].is_null());
    assert_eq!(json["events"].as_array().map(Vec::len), Some(0));
    assert_eq!(json["macs"].as_array().map(Vec::len), Some(0));

    // 已開觀測但位址無任何記錄：observed=true、presence 為 null、清單為空。
    let on = create_subnet(&state, "10.0.1.0/29").await;
    enable_observation(&state, on).await;
    let (status, json) = ip_history(&state, on, "10.0.1.3").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["observed"], true);
    assert!(json["presence"].is_null(), "空歷史的 presence 為 null");
    assert_eq!(json["events"].as_array().map(Vec::len), Some(0));
    assert_eq!(json["macs"].as_array().map(Vec::len), Some(0));

    // 非同 L2 的網段：observed=false（有效涵蓋不含遠端網段）。
    let remote = create_subnet(&state, "10.9.9.0/29").await;
    enable_observation(&state, remote).await;
    let (status, json) = ip_history(&state, remote, "10.9.9.1").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["observed"], false);

    // 不存在的網段 → 404；位址格式錯誤 → 400。
    let (status, json) = ip_history(&state, 999, "10.0.0.1").await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{json}");
    let (status, json) = ip_history(&state, on, "not-an-ip").await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "位址格式錯誤應回 400：{json}"
    );
    assert_eq!(json["error"], "validation_error");
}

#[tokio::test]
async fn mac_history_aggregates_sightings_and_links_asset_case_insensitively() {
    let pool = test_pool().await;
    let state = test_state(&pool);
    let id = create_subnet(&state, "10.0.0.0/29").await;

    // 已知 MAC：介面直寫大寫（模擬既有資料），連結仍不分大小寫。
    let asset = create_asset(
        &state,
        json!({ "property_no": "PC-001", "description": "資產甲", "location": "機房 A" }),
    )
    .await;
    let interface = create_interface(&state, asset, "aa:bb:cc:dd:ee:ff").await;
    sqlx::query("UPDATE interfaces SET mac = 'AA:BB:CC:DD:EE:FF' WHERE id = ?")
        .bind(interface)
        .execute(&pool)
        .await
        .expect("改寫介面 MAC 為大寫");

    // FF：兩個位址的事件；.7 現況較新（arp）。88：只有現況（事件已清理）。
    insert_event(
        &pool,
        id,
        "10.0.0.1",
        "aa:bb:cc:dd:ee:ff",
        "first_seen",
        "arp",
        "2026-10-01T08:00:00Z",
    )
    .await;
    insert_event(
        &pool,
        id,
        "10.0.0.7",
        "aa:bb:cc:dd:ee:ff",
        "first_seen",
        "kea_lease",
        "2026-10-02T08:00:00Z",
    )
    .await;
    insert_event(
        &pool,
        id,
        "10.0.0.9",
        "aa:00:00:00:00:99",
        "first_seen",
        "arp",
        "2026-10-03T08:00:00Z",
    )
    .await;
    insert_presence(
        &pool,
        id,
        "10.0.0.7",
        Some("2026-10-06T12:00:00Z"),
        Some("aa:bb:cc:dd:ee:ff"),
        Some("arp"),
        Some("2026-10-06T12:05:00Z"),
    )
    .await;
    insert_presence(
        &pool,
        id,
        "10.0.0.8",
        Some("2026-10-04T12:00:00Z"),
        Some("aa:00:00:00:00:88"),
        Some("kea_lease"),
        Some("2026-10-06T12:05:00Z"),
    )
    .await;

    // 已知 MAC（大寫＋連字號路徑）→ 正規化、連結資產、sightings 新到舊。
    let (status, json) = mac_history(&state, "AA-BB-CC-DD-EE-FF").await;
    assert_eq!(status, StatusCode::OK, "讀取 MAC 歷史應成功：{json}");
    assert_eq!(json["mac"], "aa:bb:cc:dd:ee:ff", "MAC 正規化為小寫冒號格式");
    assert_eq!(json["known"], true);
    assert_eq!(json["asset"]["id"], asset);
    assert_eq!(json["asset"]["description"], "資產甲");
    assert_eq!(json["asset"]["location"], "機房 A");
    assert_eq!(json["asset"]["property_no"], "PC-001");

    let sightings = json["sightings"].as_array().expect("sightings 為陣列");
    assert_eq!(sightings.len(), 2, "只含該 MAC 用過的位址：{sightings:?}");
    assert_eq!(sightings[0]["address"], "10.0.0.7");
    assert_eq!(sightings[0]["first_seen_at"], "2026-10-02T08:00:00Z");
    assert_eq!(
        sightings[0]["last_seen_at"], "2026-10-06T12:00:00Z",
        "現況較新時以現況為最後可見"
    );
    assert_eq!(sightings[0]["source"], "arp", "來源取最新訊號");
    assert_eq!(sightings[1]["address"], "10.0.0.1");
    assert_eq!(sightings[1]["first_seen_at"], "2026-10-01T08:00:00Z");
    assert_eq!(sightings[1]["last_seen_at"], "2026-10-01T08:00:00Z");
    assert_eq!(sightings[1]["source"], "arp");

    // 只有現況（事件經保留清理）：以現況為首見與最後可見。
    let (status, json) = mac_history(&state, "aa:00:00:00:00:88").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["known"], false);
    assert!(json.get("asset").is_none(), "未知 MAC 不帶資產：{json}");
    let sightings = json["sightings"].as_array().expect("sightings 為陣列");
    assert_eq!(sightings.len(), 1);
    assert_eq!(sightings[0]["address"], "10.0.0.8");
    assert_eq!(sightings[0]["first_seen_at"], "2026-10-04T12:00:00Z");
    assert_eq!(sightings[0]["last_seen_at"], "2026-10-04T12:00:00Z");
    assert_eq!(sightings[0]["source"], "kea_lease");

    // 未知 MAC（僅事件）：known=false、無資產。
    let (status, json) = mac_history(&state, "aa:00:00:00:00:99").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["known"], false);
    assert!(json.get("asset").is_none());
    assert_eq!(json["sightings"][0]["address"], "10.0.0.9");

    // 無任何紀錄的合法 MAC：known=false、空 sightings。
    let (status, json) = mac_history(&state, "aa:00:00:00:00:00").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["known"], false);
    assert_eq!(json["sightings"].as_array().map(Vec::len), Some(0));

    // 格式錯誤 → 400＋欄位標示。
    let (status, json) = mac_history(&state, "not-a-mac").await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{json}");
    assert_eq!(json["error"], "validation_error");
    assert_eq!(json["details"]["field"], "mac");
}

#[tokio::test]
async fn export_observations_returns_csv_with_bom_and_events_newest_first() {
    let pool = test_pool().await;
    let state = test_state(&pool);
    let id = create_subnet(&state, "10.0.0.0/29").await;
    enable_observation(&state, id).await;

    // 三筆事件：時間新到舊應為 03 → 02 → 01。
    for (mac, kind, source, observed_at) in [
        (
            "aa:bb:cc:dd:ee:01",
            "first_seen",
            "arp",
            "2026-10-01T08:00:00Z",
        ),
        (
            "aa:bb:cc:dd:ee:02",
            "mac_changed",
            "kea_lease",
            "2026-10-02T08:00:00Z",
        ),
        (
            "aa:bb:cc:dd:ee:03",
            "mac_changed",
            "arp",
            "2026-10-03T08:00:00Z",
        ),
    ] {
        insert_event(&pool, id, "10.0.0.1", mac, kind, source, observed_at).await;
    }

    let (status, headers, bytes) = send_bytes(
        &state,
        &format!("/api/v1/subnets/{id}/ips/10.0.0.1/observations/export"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "匯出應成功");
    assert_eq!(headers[header::CONTENT_TYPE], "text/csv; charset=utf-8");

    let date = chrono::Local::now().format("%Y%m%d");
    let disposition = headers[header::CONTENT_DISPOSITION]
        .to_str()
        .expect("Content-Disposition 為文字");
    assert!(disposition.starts_with("attachment"), "{disposition}");
    assert!(
        disposition.contains(&format!(
            "filename*=UTF-8''%E8%A7%80%E6%B8%AC%E6%AD%B7%E5%8F%B2_10.0.0.1_{date}.csv"
        )),
        "中文檔名以 filename* 編碼：{disposition}"
    );

    // UTF-8 BOM＋標題列＋資料列（新到舊）。
    assert_eq!(&bytes[..3], &[0xEF, 0xBB, 0xBF], "回應須有 UTF-8 BOM");
    let mut reader = csv::Reader::from_reader(&bytes[3..]);
    let headers = reader.headers().expect("標題列").clone();
    assert_eq!(
        headers.iter().collect::<Vec<_>>(),
        ["address", "mac", "kind", "source", "observed_at"]
    );
    let rows: Vec<Vec<String>> = reader
        .records()
        .map(|record| record.expect("資料列").iter().map(str::to_string).collect())
        .collect();
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0][0], "10.0.0.1");
    assert_eq!(rows[0][1], "aa:bb:cc:dd:ee:03");
    assert_eq!(rows[0][2], "mac_changed");
    assert_eq!(rows[0][3], "arp");
    assert_eq!(rows[0][4], "2026-10-03T08:00:00Z");
    assert_eq!(rows[1][1], "aa:bb:cc:dd:ee:02", "第二列為次新事件");
    assert_eq!(rows[2][1], "aa:bb:cc:dd:ee:01");
    assert_eq!(rows[2][2], "first_seen");

    // 空歷史：僅標題列、仍為 200。
    let (status, _, bytes) = send_bytes(
        &state,
        &format!("/api/v1/subnets/{id}/ips/10.0.0.2/observations/export"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let mut reader = csv::Reader::from_reader(&bytes[3..]);
    assert_eq!(reader.headers().expect("標題列").len(), 5);
    assert_eq!(reader.records().count(), 0, "空歷史僅標題列");

    // 不存在的網段 → 404。
    let (status, _, _) = send_bytes(
        &state,
        "/api/v1/subnets/999/ips/10.0.0.1/observations/export",
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
