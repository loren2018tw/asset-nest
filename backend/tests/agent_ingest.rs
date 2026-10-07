//! 觀測代理入庫整合測試（見票 02、票 03、spec §HTTP API、ADR-0016／0017）。
//!
//! 覆蓋：sweep 的 checked／seen 寫入、passive 只收 CIDR 外 sender（旗標與
//! 來源 `arp_passive`）、事件推導與重複不重寫、時間排序與未來夾制、混批
//! 同交易套用、未對應網段只記代理狀態、認證與未設碼沿用心跳行為、
//! 「有在線代理」的未觀測判定（含代理過期回到未觀測）。
//! 記憶體 SQLite＋`sqlx::migrate!`＋`tower::ServiceExt::oneshot`；連線來源以
//! 注入 `ConnectInfo` 模擬（比照 `agent_status.rs`）；loopback 來源另注入
//! `X-Forwarded-For`，驗證本機反向代理情境的來源 IP 判定（見 spec §4）。

use std::net::SocketAddr;
use std::path::PathBuf;

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Method, Request, StatusCode, header};
use chrono::{Duration, Utc};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use sqlx::SqlitePool;
use sqlx::sqlite::SqlitePoolOptions;
use tower::ServiceExt;

use asset_nest::observation::timestamp;
use asset_nest::{AppState, app};

/// 測試用共用認證碼。
const AUTH_CODE: &str = "agent-secret-02";
/// 測試連線來源（TEST-NET-3；非 loopback，即 `source_ip`，偽造 XFF 不採信）。
const REMOTE: &str = "203.0.113.9:40123";
/// 本機反向代理（nginx）情境的連線來源（loopback；帶 XFF 時採第一段）。
const LOOPBACK: &str = "127.0.0.1:40124";
/// 測試用代理 instance id。
const AGENT_ID: &str = "b3f1a2c4-0000-4000-8000-000000000201";

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

fn dist_dir() -> PathBuf {
    std::env::temp_dir().join("asset-nest-test-no-dist")
}

/// 建立已設認證碼與在線門檻的 AppState。
fn test_state(pool: &SqlitePool) -> AppState {
    AppState::new(pool.clone(), dist_dir())
        .with_agent_auth_code(Some(AUTH_CODE.to_string()))
        .with_agent_stale_secs(900)
}

/// 以 `oneshot` 發送請求（附連線來源）；回傳狀態碼與 JSON。
async fn send(
    state: &AppState,
    method: Method,
    uri: &str,
    body: Option<Value>,
    auth_code: Option<&str>,
) -> (StatusCode, Value) {
    send_from(state, method, uri, REMOTE, None, body, auth_code).await
}

/// 以 `oneshot` 發送請求，可指定連線來源與 `X-Forwarded-For`：loopback
/// 來源比照本機反向代理（nginx）注入 XFF（見 spec §4）。
async fn send_from(
    state: &AppState,
    method: Method,
    uri: &str,
    remote: &str,
    forwarded_for: Option<&str>,
    body: Option<Value>,
    auth_code: Option<&str>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(code) = auth_code {
        builder = builder.header("x-auth-code", code);
    }
    if let Some(value) = forwarded_for {
        builder = builder.header("x-forwarded-for", value);
    }
    let body = match body {
        Some(value) => {
            builder = builder.header(header::CONTENT_TYPE, "application/json");
            Body::from(value.to_string())
        }
        None => Body::empty(),
    };

    let mut request = builder.body(body).expect("建立請求");
    request
        .extensions_mut()
        .insert(ConnectInfo(remote.parse::<SocketAddr>().expect("來源位址")));

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

/// 送出 sweep 觀測回報（帶測試認證碼）；回傳狀態碼與 JSON。
async fn observations(state: &AppState, body: Value) -> (StatusCode, Value) {
    send(
        state,
        Method::POST,
        "/api/v1/agents/observations",
        Some(body),
        Some(AUTH_CODE),
    )
    .await
}

/// 新增網段並斷言成功，回傳 id。
async fn create_subnet(state: &AppState, cidr: &str, name: &str) -> i64 {
    let (status, json) = send(
        state,
        Method::POST,
        "/api/v1/subnets",
        Some(json!({ "cidr": cidr, "name": name })),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "新增網段應成功：{json}");
    json["id"].as_i64().expect("回應含 id")
}

/// 相對現在產生報告時間（不依賴測試機器時鐘與固定日曆的相對關係）；
/// 格式與資料庫一致。
fn ago(hours: i64) -> String {
    timestamp(Utc::now() - Duration::hours(hours))
}

/// 單筆 sweep 回報的最小 body。
fn sweep_body(
    instance_id: &str,
    cidr: &str,
    observed_at: &str,
    checked: &[&str],
    seen: &[(&str, &str)],
) -> Value {
    let seen: Vec<Value> = seen
        .iter()
        .map(|(address, mac)| json!({ "address": address, "mac": mac }))
        .collect();

    json!({
        "instance_id": instance_id,
        "name": "edge-agent",
        "version": "0.1.0",
        "subnet_cidr": cidr,
        "reports": [{
            "kind": "sweep",
            "observed_at": observed_at,
            "checked": checked,
            "seen": seen,
        }]
    })
}

/// 單筆 passive 回報的最小 body（見票 03）。
fn passive_body(
    instance_id: &str,
    cidr: &str,
    observed_at: &str,
    senders: &[(&str, &str)],
) -> Value {
    let senders: Vec<Value> = senders
        .iter()
        .map(|(address, mac)| json!({ "address": address, "mac": mac }))
        .collect();

    json!({
        "instance_id": instance_id,
        "name": "edge-agent",
        "version": "0.1.0",
        "subnet_cidr": cidr,
        "reports": [{
            "kind": "passive",
            "observed_at": observed_at,
            "senders": senders,
        }]
    })
}

/// 讀取現況列（最後可見、MAC、來源、最後檢查）。
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
           FROM ip_presence
          WHERE subnet_id = ? AND address = ?",
    )
    .bind(subnet_id)
    .bind(address)
    .fetch_optional(pool)
    .await
    .expect("查詢觀測現況")
}

/// 讀取現況列的網段外旗標（被動路徑斷言用；列必須存在）。
async fn out_of_subnet_flag(pool: &SqlitePool, subnet_id: i64, address: &str) -> i64 {
    sqlx::query_scalar("SELECT out_of_subnet FROM ip_presence WHERE subnet_id = ? AND address = ?")
        .bind(subnet_id)
        .bind(address)
        .fetch_one(pool)
        .await
        .expect("現況列存在")
}

/// 讀取全部事件（舊到新）。
async fn events(pool: &SqlitePool) -> Vec<(String, String, String, String)> {
    sqlx::query_as("SELECT address, mac, kind, source FROM observation_event ORDER BY id ASC")
        .fetch_all(pool)
        .await
        .expect("查詢觀測事件")
}

/// 資料表列數（驗證「不得寫入」用；表名為測試內部常數）。
async fn count(pool: &SqlitePool, table: &str) -> i64 {
    sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
        .fetch_one(pool)
        .await
        .expect("查詢列數")
}

/// 讀取某網段 IP 清單。
async fn list_ips(state: &AppState, subnet_id: i64) -> Value {
    let (status, page) = send(
        state,
        Method::GET,
        &format!("/api/v1/subnets/{subnet_id}/ips?per_page=50"),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "讀取 IP 清單：{page}");
    page
}

/// 讀取某位址的觀測歷史（詳情）。
async fn ip_history(state: &AppState, subnet_id: i64, address: &str) -> Value {
    let (status, history) = send(
        state,
        Method::GET,
        &format!("/api/v1/subnets/{subnet_id}/ips/{address}/observations"),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "讀取 IP 歷史：{history}");
    history
}

/// 列的 `observed` 旗標（必須為布林）。
fn observed_of(item: &Value) -> bool {
    item["observed"].as_bool().expect("observed 為布林")
}

#[tokio::test]
async fn sweep_report_stores_checked_seen_and_updates_agent() {
    let pool = test_pool().await;
    let state = test_state(&pool);
    let subnet_id = create_subnet(&state, "10.20.0.0/24", "代理區").await;

    let observed_at = ago(2);
    let (status, body) = observations(
        &state,
        sweep_body(
            AGENT_ID,
            "10.20.0.5/24", // host bits 未收斂：正規化後精確對應
            &observed_at,
            &["10.20.0.1", "10.20.0.2"],
            &[("10.20.0.2", "AA-BB-CC-DD-EE-02")],
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "回報應成功：{body}");
    assert_eq!(body["stored"], true);
    assert!(body.get("reason").is_none(), "成功不含 reason");

    // checked：更新最後檢查、不寫最後可見
    let checked = presence(&pool, subnet_id, "10.20.0.1")
        .await
        .expect("checked 列存在");
    assert_eq!(
        checked.3.as_deref(),
        Some(observed_at.as_str()),
        "last_checked_at 為報告時間"
    );
    assert!(checked.0.is_none(), "僅檢查不算看到");
    assert!(checked.1.is_none());
    assert!(checked.2.is_none());

    // seen：更新最後可見、MAC 正規化小寫冒號、來源 arp
    let seen = presence(&pool, subnet_id, "10.20.0.2")
        .await
        .expect("seen 列存在");
    assert_eq!(seen.0.as_deref(), Some(observed_at.as_str()));
    assert_eq!(seen.1.as_deref(), Some("aa:bb:cc:dd:ee:02"));
    assert_eq!(seen.2.as_deref(), Some("arp"));
    assert_eq!(seen.3.as_deref(), Some(observed_at.as_str()));

    // 事件：first_seen（來源 arp、報告時間）
    assert_eq!(
        events(&pool).await,
        vec![(
            "10.20.0.2".to_string(),
            "aa:bb:cc:dd:ee:02".to_string(),
            "first_seen".to_string(),
            "arp".to_string(),
        )]
    );

    // 代理狀態：回報算一次回報（在線）、CIDR 正規化、最後觀測時間寫入
    let (status, page) = send(&state, Method::GET, "/api/v1/agents", None, None).await;
    assert_eq!(status, StatusCode::OK);
    let agent = &page["items"][0];
    assert_eq!(agent["instance_id"], AGENT_ID);
    assert_eq!(agent["subnet_cidr"], "10.20.0.0/24", "CIDR 須正規化入庫");
    assert_eq!(agent["subnet_id"], subnet_id);
    assert_eq!(agent["online"], true, "回報亦更新 last_report_at");
    assert!(
        agent["last_observation_at"].is_string(),
        "成功後更新最後觀測時間"
    );
}

#[tokio::test]
async fn repeated_reports_do_not_duplicate_events_and_mac_change_writes_once() {
    let pool = test_pool().await;
    let state = test_state(&pool);
    let subnet_id = create_subnet(&state, "10.21.0.0/24", "事件區").await;

    let mac_old = "aa:bb:cc:dd:ee:07";
    let mac_new = "aa:bb:cc:dd:ee:77";
    let at_first = ago(4);
    let at_second = ago(3);
    let at_third = ago(2);
    let at_fourth = ago(1);

    // 同 MAC 重複兩次（含大小寫／分隔符變化）→ 只寫一次 first_seen；
    // MAC 變更後再重複 → 只寫一次 mac_changed。
    for (at, mac) in [
        (&at_first, "AA:BB:CC:DD:EE:07"),
        (&at_second, mac_old),
        (&at_third, "AA-BB-CC-DD-EE-77"),
        (&at_fourth, mac_new),
    ] {
        let (status, body) = observations(
            &state,
            sweep_body(
                AGENT_ID,
                "10.21.0.0/24",
                at,
                &["10.21.0.7"],
                &[("10.21.0.7", mac)],
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "回報應成功：{body}");
        assert_eq!(body["stored"], true);
    }

    let seen = presence(&pool, subnet_id, "10.21.0.7")
        .await
        .expect("現況列存在");
    assert_eq!(seen.1.as_deref(), Some(mac_new), "現況 MAC 取最新報告");
    assert_eq!(seen.0.as_deref(), Some(at_fourth.as_str()));

    assert_eq!(
        events(&pool).await,
        vec![
            (
                "10.21.0.7".to_string(),
                mac_old.to_string(),
                "first_seen".to_string(),
                "arp".to_string(),
            ),
            (
                "10.21.0.7".to_string(),
                mac_new.to_string(),
                "mac_changed".to_string(),
                "arp".to_string(),
            ),
        ],
        "同 MAC 不重複寫事件；MAC 變更恰寫一次"
    );
}

#[tokio::test]
async fn reports_apply_in_time_order_and_future_is_clamped() {
    let pool = test_pool().await;
    let state = test_state(&pool);
    let subnet_id = create_subnet(&state, "10.22.0.0/24", "時間區").await;

    let earlier = ago(3);
    let later = ago(2);

    // 報告以「晚→早」亂序送出；排序套用後事件須為 first_seen→mac_changed。
    let (status, response) = observations(
        &state,
        json!({
            "instance_id": AGENT_ID,
            "name": "edge-agent",
            "version": "0.1.0",
            "subnet_cidr": "10.22.0.0/24",
            "reports": [
                {
                    "kind": "sweep",
                    "observed_at": later,
                    "checked": ["10.22.0.9"],
                    "seen": [{ "address": "10.22.0.9", "mac": "aa:bb:cc:dd:ee:99" }]
                },
                {
                    "kind": "sweep",
                    "observed_at": earlier,
                    "checked": ["10.22.0.9"],
                    "seen": [{ "address": "10.22.0.9", "mac": "aa:bb:cc:dd:ee:09" }]
                }
            ]
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{response}");

    assert_eq!(
        events(&pool).await,
        vec![
            (
                "10.22.0.9".to_string(),
                "aa:bb:cc:dd:ee:09".to_string(),
                "first_seen".to_string(),
                "arp".to_string(),
            ),
            (
                "10.22.0.9".to_string(),
                "aa:bb:cc:dd:ee:99".to_string(),
                "mac_changed".to_string(),
                "arp".to_string(),
            ),
        ],
        "依 observed_at 舊到新套用：早報告先、晚報告後"
    );
    let seen = presence(&pool, subnet_id, "10.22.0.9")
        .await
        .expect("現況列存在");
    assert_eq!(seen.0.as_deref(), Some(later.as_str()), "最新報告勝出");
    assert_eq!(seen.1.as_deref(), Some("aa:bb:cc:dd:ee:99"));
    assert_eq!(
        seen.3.as_deref(),
        Some(later.as_str()),
        "last_checked_at 為最新報告時間"
    );

    // 較舊的離線補送不覆寫較新的最後檢查／最後可見，也不寫事件（latest-wins）
    let stale = ago(5);
    let (status, response) = observations(
        &state,
        sweep_body(
            AGENT_ID,
            "10.22.0.0/24",
            &stale,
            &["10.22.0.9"],
            &[("10.22.0.9", "aa:bb:cc:dd:ee:09")],
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{response}");
    let seen = presence(&pool, subnet_id, "10.22.0.9")
        .await
        .expect("現況列存在");
    assert_eq!(
        seen.3.as_deref(),
        Some(later.as_str()),
        "較舊補送不覆寫 last_checked_at"
    );
    assert_eq!(
        seen.0.as_deref(),
        Some(later.as_str()),
        "較舊補送不覆寫 last_seen_at"
    );
    assert_eq!(
        seen.1.as_deref(),
        Some("aa:bb:cc:dd:ee:99"),
        "較舊補送不覆寫 MAC"
    );
    assert_eq!(events(&pool).await.len(), 2, "較舊補送不寫事件");

    // 未來 observed_at：夾到「現在」（界於請求前後時間之間）
    let before = timestamp(Utc::now());
    let (status, response) = observations(
        &state,
        sweep_body(
            AGENT_ID,
            "10.22.0.0/24",
            "2099-01-01T00:00:00Z",
            &["10.22.0.10"],
            &[("10.22.0.10", "aa:bb:cc:dd:ee:10")],
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{response}");
    let after = timestamp(Utc::now());

    let clamped = presence(&pool, subnet_id, "10.22.0.10")
        .await
        .expect("現況列存在");
    let seen_at = clamped.0.as_deref().expect("last_seen_at");
    assert!(
        before.as_str() <= seen_at && seen_at <= after.as_str(),
        "未來值夾到現在：{seen_at}（{before} ～ {after}）"
    );
    assert_eq!(
        clamped.3.as_deref(),
        Some(seen_at),
        "checked 與 seen 同夾制時間"
    );
}

#[tokio::test]
async fn unmatched_subnet_records_agent_state_only() {
    let pool = test_pool().await;
    let state = test_state(&pool);

    let (status, body) = observations(
        &state,
        sweep_body(
            AGENT_ID,
            "10.99.0.0/24",
            &ago(1),
            &["10.99.0.1"],
            &[("10.99.0.1", "aa:bb:cc:dd:ee:01")],
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "未對應仍應回應：{body}");
    assert_eq!(body["stored"], false);
    assert_eq!(body["reason"], "subnet_unmatched");

    // 代理狀態照記（算一次回報）；觀測不入庫
    let (status, page) = send(&state, Method::GET, "/api/v1/agents", None, None).await;
    assert_eq!(status, StatusCode::OK);
    let agent = &page["items"][0];
    assert!(agent["subnet_id"].is_null(), "對不到網段須留空");
    assert_eq!(agent["online"], true, "未對應不影響在線判定");
    assert!(agent["last_report_at"].is_string());
    assert!(
        agent["last_observation_at"].is_null(),
        "未入庫不得更新最後觀測時間"
    );

    assert_eq!(count(&pool, "ip_presence").await, 0, "未對應不得留下現況列");
    assert_eq!(
        count(&pool, "observation_event").await,
        0,
        "未對應不得寫事件"
    );
}

#[tokio::test]
async fn auth_matches_heartbeat_and_missing_code_returns_503() {
    let pool = test_pool().await;
    let state = test_state(&pool);
    let _ = create_subnet(&state, "10.23.0.0/24", "認證區").await;

    let body = sweep_body(
        AGENT_ID,
        "10.23.0.0/24",
        &ago(1),
        &["10.23.0.1"],
        &[("10.23.0.2", "aa:bb:cc:dd:ee:02")],
    );

    // 錯碼：401、不寫代理與觀測、累計被拒回報（自報名稱／版本 best-effort）
    let (status, response) = send(
        &state,
        Method::POST,
        "/api/v1/agents/observations",
        Some(body.clone()),
        Some("wrong-code"),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "錯碼須拒收：{response}");
    assert_eq!(response["error"], "unauthorized");
    assert_eq!(count(&pool, "agent").await, 0, "錯碼不得寫代理");
    assert_eq!(count(&pool, "ip_presence").await, 0, "錯碼不得寫觀測");

    let (status, page) = send(
        &state,
        Method::GET,
        "/api/v1/agents/auth-failures",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let failure = &page["items"][0];
    assert_eq!(failure["source_ip"], "203.0.113.9", "來源 IP 取連線來源");
    assert_eq!(failure["claimed_name"], "edge-agent");
    assert_eq!(failure["claimed_version"], "0.1.0");
    assert_eq!(failure["attempt_count"], 1);

    // 未設碼：503、不算認證失敗（與票 01 心跳一致）
    let unset = AppState::new(pool.clone(), dist_dir());
    let (status, response) = send(
        &unset,
        Method::POST,
        "/api/v1/agents/observations",
        Some(body),
        Some(AUTH_CODE),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::SERVICE_UNAVAILABLE,
        "未設碼須 503：{response}"
    );
    assert_eq!(response["error"], "service_unavailable");
    assert_eq!(
        count(&pool, "agent_auth_failure").await,
        1,
        "503 不是認證失敗，不得累計"
    );
    assert_eq!(count(&pool, "agent").await, 0, "503 不得寫代理");
}

#[tokio::test]
async fn unknown_report_kind_is_rejected() {
    let pool = test_pool().await;
    let state = test_state(&pool);
    let _ = create_subnet(&state, "10.26.0.0/24", "種類區").await;

    let (status, body) = observations(
        &state,
        json!({
            "instance_id": AGENT_ID,
            "name": "edge-agent",
            "version": "0.1.0",
            "subnet_cidr": "10.26.0.0/24",
            "reports": [{
                "kind": "mystery",
                "observed_at": ago(1),
                "checked": [],
                "seen": []
            }]
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "未知 kind 須 400：{body}");
    assert_eq!(body["error"], "validation_error");
    assert_eq!(body["details"]["field"], "reports[0].kind");

    assert_eq!(count(&pool, "agent").await, 0, "拒收不得寫代理");
    assert_eq!(count(&pool, "ip_presence").await, 0, "拒收不得寫觀測");
}

#[tokio::test]
async fn passive_report_stores_only_out_of_subnet_senders_and_lists_them() {
    let pool = test_pool().await;
    let state = test_state(&pool);
    let subnet_id = create_subnet(&state, "10.30.0.0/24", "被動區").await;

    // 已知 MAC 連結資產（沿用既有介面；清單應自動反映）。
    let (status, asset) = send(
        &state,
        Method::POST,
        "/api/v1/assets",
        Some(json!({ "property_no": "PC-303", "description": "被動設備", "location": "被動區" })),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "新增資產：{asset}");
    let asset_id = asset["id"].as_i64().expect("資產 id");
    let (status, _) = send(
        &state,
        Method::POST,
        &format!("/api/v1/assets/{asset_id}/interfaces"),
        Some(json!({ "name": "eth0", "mac": "aa:bb:cc:dd:ee:88" })),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let observed_at = ago(2);
    let (status, body) = observations(
        &state,
        passive_body(
            AGENT_ID,
            "10.30.0.0/24",
            &observed_at,
            &[
                ("10.30.0.5", "AA-BB-CC-DD-EE-05"),   // CIDR 內：丟棄
                ("10.30.0.0", "aa:bb:cc:dd:ee:00"),   // 網段位址：屬網段內
                ("192.168.9.8", "AA-BB-CC-DD-EE-88"), // CIDR 外、已知 MAC
                ("192.168.9.9", "AA-BB-CC-DD-EE-99"), // CIDR 外、未知 MAC
            ],
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "passive 回報應成功：{body}");
    assert_eq!(body["stored"], true);
    assert!(body.get("reason").is_none());

    // CIDR 外：寫入現況（out_of_subnet=1、來源 arp_passive、MAC 正規化）
    let known = presence(&pool, subnet_id, "192.168.9.8")
        .await
        .expect("CIDR 外已知列存在");
    assert_eq!(known.0.as_deref(), Some(observed_at.as_str()));
    assert_eq!(known.1.as_deref(), Some("aa:bb:cc:dd:ee:88"));
    assert_eq!(known.2.as_deref(), Some("arp_passive"));
    assert!(known.3.is_none(), "被動不更新最後檢查");
    assert_eq!(out_of_subnet_flag(&pool, subnet_id, "192.168.9.8").await, 1);

    let unknown = presence(&pool, subnet_id, "192.168.9.9")
        .await
        .expect("CIDR 外未知列存在");
    assert_eq!(unknown.1.as_deref(), Some("aa:bb:cc:dd:ee:99"));
    assert_eq!(out_of_subnet_flag(&pool, subnet_id, "192.168.9.9").await, 1);

    // CIDR 內：完全不寫（現況與事件皆無）
    assert!(
        presence(&pool, subnet_id, "10.30.0.5").await.is_none(),
        "CIDR 內 sender 丟棄"
    );
    assert!(
        presence(&pool, subnet_id, "10.30.0.0").await.is_none(),
        "網段位址屬 CIDR 內"
    );
    assert_eq!(count(&pool, "ip_presence").await, 2, "只寫 CIDR 外 sender");
    assert_eq!(
        events(&pool).await,
        vec![
            (
                "192.168.9.8".to_string(),
                "aa:bb:cc:dd:ee:88".to_string(),
                "first_seen".to_string(),
                "arp_passive".to_string(),
            ),
            (
                "192.168.9.9".to_string(),
                "aa:bb:cc:dd:ee:99".to_string(),
                "first_seen".to_string(),
                "arp_passive".to_string(),
            ),
        ],
        "CIDR 外首見事件來源 arp_passive"
    );

    // E2E：「網段外觀測」清單看得到新資料（含未登錄與已知資產連結）
    let (status, list) = send(
        &state,
        Method::GET,
        "/api/v1/observations/out-of-subnet",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "讀取網段外觀測清單：{list}");
    let items = list["items"].as_array().expect("items 為陣列");
    assert_eq!(items.len(), 2, "只列 out_of_subnet=1：{items:?}");

    // 同時間依位址升冪：9.8 在 9.9 前。
    assert_eq!(items[0]["address"], "192.168.9.8");
    assert_eq!(items[0]["subnet_id"], subnet_id);
    assert_eq!(items[0]["subnet_cidr"], "10.30.0.0/24");
    assert_eq!(items[0]["subnet_name"], "被動區");
    assert_eq!(items[0]["last_seen_at"], observed_at);
    assert_eq!(items[0]["first_seen_at"], observed_at, "首見取最早事件時間");
    assert_eq!(items[0]["source"], "arp_passive");
    assert_eq!(items[0]["known"], true, "已知 MAC 連結資產");
    assert_eq!(items[0]["asset"]["id"], asset_id);

    assert_eq!(items[1]["address"], "192.168.9.9");
    assert_eq!(items[1]["known"], false, "未登錄 MAC");
    assert!(
        items[1].get("asset").is_none(),
        "未登錄 MAC 不帶資產：{:?}",
        items[1]
    );
}

#[tokio::test]
async fn repeated_passive_reports_write_no_extra_events_and_respect_latest_wins() {
    let pool = test_pool().await;
    let state = test_state(&pool);
    let subnet_id = create_subnet(&state, "10.32.0.0/24", "被動重複區").await;

    let mac_old = "aa:bb:cc:dd:ee:31";
    let mac_new = "aa:bb:cc:dd:ee:32";
    let at_first = ago(4);
    let at_second = ago(3);
    let at_third = ago(2);
    let at_fourth = ago(1);

    // 同 MAC 重複兩次（含大小寫／分隔符變化）→ 只寫一次 first_seen；
    // MAC 變更後再重複 → 只寫一次 mac_changed。
    for (at, mac) in [
        (&at_first, "AA:BB:CC:DD:EE:31"),
        (&at_second, mac_old),
        (&at_third, "AA-BB-CC-DD-EE-32"),
        (&at_fourth, mac_new),
    ] {
        let (status, body) = observations(
            &state,
            passive_body(AGENT_ID, "10.32.0.0/24", at, &[("192.168.9.31", mac)]),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "回報應成功：{body}");
        assert_eq!(body["stored"], true);
    }

    let seen = presence(&pool, subnet_id, "192.168.9.31")
        .await
        .expect("現況列存在");
    assert_eq!(seen.0.as_deref(), Some(at_fourth.as_str()), "最新回報勝出");
    assert_eq!(seen.1.as_deref(), Some(mac_new));
    assert_eq!(seen.2.as_deref(), Some("arp_passive"));
    assert_eq!(
        out_of_subnet_flag(&pool, subnet_id, "192.168.9.31").await,
        1
    );

    assert_eq!(
        events(&pool).await,
        vec![
            (
                "192.168.9.31".to_string(),
                mac_old.to_string(),
                "first_seen".to_string(),
                "arp_passive".to_string(),
            ),
            (
                "192.168.9.31".to_string(),
                mac_new.to_string(),
                "mac_changed".to_string(),
                "arp_passive".to_string(),
            ),
        ],
        "同 MAC 不重複寫事件；MAC 變更恰寫一次"
    );

    // 較舊的離線補送不覆寫現況、也不寫事件（latest-wins）
    let stale = ago(6);
    let (status, body) = observations(
        &state,
        passive_body(
            AGENT_ID,
            "10.32.0.0/24",
            &stale,
            &[("192.168.9.31", "aa:bb:cc:dd:ee:30")],
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let seen = presence(&pool, subnet_id, "192.168.9.31")
        .await
        .expect("現況列存在");
    assert_eq!(seen.0.as_deref(), Some(at_fourth.as_str()));
    assert_eq!(seen.1.as_deref(), Some(mac_new), "較舊補送不覆寫 MAC");
    assert_eq!(events(&pool).await.len(), 2, "較舊補送不寫事件");
}

#[tokio::test]
async fn sweep_and_passive_reports_apply_together_in_one_transaction() {
    let pool = test_pool().await;
    let state = test_state(&pool);
    let subnet_id = create_subnet(&state, "10.33.0.0/24", "混批區").await;

    let sweep_at = ago(1);
    let passive_at = ago(3);

    // 同一請求混批（sweep 較晚、passive 較早且亂序送出）：由 observed_at
    // 排序後在同一交易套用，兩者皆落地。
    let (status, body) = observations(
        &state,
        json!({
            "instance_id": AGENT_ID,
            "name": "edge-agent",
            "version": "0.1.0",
            "subnet_cidr": "10.33.0.0/24",
            "reports": [
                {
                    "kind": "sweep",
                    "observed_at": sweep_at,
                    "checked": ["10.33.0.1"],
                    "seen": [{ "address": "10.33.0.2", "mac": "AA:BB:CC:DD:EE:02" }]
                },
                {
                    "kind": "passive",
                    "observed_at": passive_at,
                    "senders": [{ "address": "192.168.9.33", "mac": "AA-BB-CC-DD-EE:33" }]
                }
            ]
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "混批回報應成功：{body}");
    assert_eq!(body["stored"], true);

    // sweep：checked 與 seen 落地
    let checked = presence(&pool, subnet_id, "10.33.0.1")
        .await
        .expect("checked 列存在");
    assert_eq!(checked.3.as_deref(), Some(sweep_at.as_str()));
    assert!(checked.0.is_none(), "僅檢查不算看到");
    let seen = presence(&pool, subnet_id, "10.33.0.2")
        .await
        .expect("seen 列存在");
    assert_eq!(seen.0.as_deref(), Some(sweep_at.as_str()));
    assert_eq!(seen.1.as_deref(), Some("aa:bb:cc:dd:ee:02"));
    assert_eq!(seen.2.as_deref(), Some("arp"));

    // passive：CIDR 外落地且帶旗標（MAC 正規化）
    let outside = presence(&pool, subnet_id, "192.168.9.33")
        .await
        .expect("網段外列存在");
    assert_eq!(outside.0.as_deref(), Some(passive_at.as_str()));
    assert_eq!(outside.1.as_deref(), Some("aa:bb:cc:dd:ee:33"));
    assert_eq!(outside.2.as_deref(), Some("arp_passive"));
    assert_eq!(
        out_of_subnet_flag(&pool, subnet_id, "192.168.9.33").await,
        1
    );

    // 事件：passive（較早）先寫、sweep（較晚）後寫。
    assert_eq!(
        events(&pool).await,
        vec![
            (
                "192.168.9.33".to_string(),
                "aa:bb:cc:dd:ee:33".to_string(),
                "first_seen".to_string(),
                "arp_passive".to_string(),
            ),
            (
                "10.33.0.2".to_string(),
                "aa:bb:cc:dd:ee:02".to_string(),
                "first_seen".to_string(),
                "arp".to_string(),
            ),
        ],
        "混批依 observed_at 排序套用"
    );

    // 代理狀態：算一次回報且更新最後觀測時間。
    let (status, page) = send(&state, Method::GET, "/api/v1/agents", None, None).await;
    assert_eq!(status, StatusCode::OK);
    let agent = &page["items"][0];
    assert_eq!(agent["online"], true);
    assert!(agent["last_observation_at"].is_string());

    // E2E：網段外觀測清單只含被動列，不含 sweep 的 CIDR 內列。
    let (status, list) = send(
        &state,
        Method::GET,
        "/api/v1/observations/out-of-subnet",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let items = list["items"].as_array().expect("items 為陣列");
    assert_eq!(items.len(), 1, "只列 CIDR 外被動列：{items:?}");
    assert_eq!(items[0]["address"], "192.168.9.33");
}

#[tokio::test]
async fn validation_rejects_invalid_input_without_writes() {
    let pool = test_pool().await;
    let state = test_state(&pool);
    let _ = create_subnet(&state, "10.24.0.0/24", "驗證區").await;

    let cases: [(Value, &str, &str); 12] = [
        (json!({}), "instance_id", "缺漏 instance_id"),
        (
            json!({ "instance_id": "  " }),
            "instance_id",
            "空白 instance_id",
        ),
        (
            json!({ "instance_id": "id-1", "name": "" }),
            "name",
            "空 name",
        ),
        (
            json!({ "instance_id": "id-1", "name": "n", "version": " " }),
            "version",
            "空白 version",
        ),
        (
            json!({
                "instance_id": "id-1", "name": "n", "version": "v",
                "subnet_cidr": "not-a-cidr"
            }),
            "subnet_cidr",
            "非法 CIDR",
        ),
        (
            json!({
                "instance_id": "id-1", "name": "n", "version": "v",
                "subnet_cidr": "10.24.0.0/24",
                "reports": [{ "observed_at": "2026-10-05T10:00:00Z", "checked": [], "seen": [] }]
            }),
            "reports[0].kind",
            "缺漏 kind",
        ),
        (
            json!({
                "instance_id": "id-1", "name": "n", "version": "v",
                "subnet_cidr": "10.24.0.0/24",
                "reports": [{
                    "kind": "sweep", "observed_at": "2026-10-05 10:00:00",
                    "checked": [], "seen": []
                }]
            }),
            "reports[0].observed_at",
            "非法 observed_at 格式",
        ),
        (
            json!({
                "instance_id": "id-1", "name": "n", "version": "v",
                "subnet_cidr": "10.24.0.0/24",
                "reports": [{
                    "kind": "sweep", "observed_at": "2026-10-05T10:00:00Z",
                    "checked": ["fd00::1"], "seen": []
                }]
            }),
            "reports[0].checked[0]",
            "checked 非 IPv4",
        ),
        (
            json!({
                "instance_id": "id-1", "name": "n", "version": "v",
                "subnet_cidr": "10.24.0.0/24",
                "reports": [{
                    "kind": "sweep", "observed_at": "2026-10-05T10:00:00Z",
                    "checked": [],
                    "seen": [{ "address": "10.24.0.300", "mac": "aa:bb:cc:dd:ee:01" }]
                }]
            }),
            "reports[0].seen[0].address",
            "非法 seen 位址",
        ),
        (
            json!({
                "instance_id": "id-1", "name": "n", "version": "v",
                "subnet_cidr": "10.24.0.0/24",
                "reports": [{
                    "kind": "sweep", "observed_at": "2026-10-05T10:00:00Z",
                    "checked": [],
                    "seen": [{ "address": "10.24.0.1", "mac": "not-a-mac" }]
                }]
            }),
            "reports[0].seen[0].mac",
            "非法 MAC",
        ),
        (
            json!({
                "instance_id": "id-1", "name": "n", "version": "v",
                "subnet_cidr": "10.24.0.0/24",
                "reports": [{
                    "kind": "passive", "observed_at": "2026-10-05T10:00:00Z",
                    "senders": [{ "address": "fd00::1", "mac": "aa:bb:cc:dd:ee:01" }]
                }]
            }),
            "reports[0].senders[0].address",
            "passive sender 非 IPv4",
        ),
        (
            json!({
                "instance_id": "id-1", "name": "n", "version": "v",
                "subnet_cidr": "10.24.0.0/24",
                "reports": [{
                    "kind": "passive", "observed_at": "2026-10-05T10:00:00Z",
                    "senders": [{ "address": "192.168.9.9", "mac": "not-a-mac" }]
                }]
            }),
            "reports[0].senders[0].mac",
            "passive sender 非法 MAC",
        ),
    ];

    for (body, field, label) in cases {
        let (status, response) = observations(&state, body).await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "{label} 應回 400：{response}"
        );
        assert_eq!(response["error"], "validation_error", "{label}");
        assert_eq!(
            response["details"]["field"], field,
            "{label} 須標示欄位：{response}"
        );
    }

    assert_eq!(count(&pool, "agent").await, 0, "驗證失敗不得寫代理");
    assert_eq!(count(&pool, "ip_presence").await, 0, "驗證失敗不得寫觀測");
}

#[tokio::test]
async fn online_agent_marks_subnet_observed_until_report_expires() {
    let pool = test_pool().await;
    let state = test_state(&pool);
    let subnet_id = create_subnet(&state, "10.25.0.0/29", "代理涵蓋區").await;

    // 尚無代理：未觀測
    let page = list_ips(&state, subnet_id).await;
    assert!(
        page["items"]
            .as_array()
            .expect("items")
            .iter()
            .all(|item| !observed_of(item)),
        "無代理：未觀測"
    );

    // sweep 回報後代理上線：該網段清單與詳情皆視為已觀測
    let (status, body) = observations(
        &state,
        sweep_body(
            AGENT_ID,
            "10.25.0.0/29",
            &ago(1),
            &["10.25.0.1"],
            &[("10.25.0.2", "aa:bb:cc:dd:ee:02")],
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let page = list_ips(&state, subnet_id).await;
    let items = page["items"].as_array().expect("items");
    assert!(
        items.iter().all(observed_of),
        "有在線代理：清單所有列已觀測"
    );
    let entry = items
        .iter()
        .find(|item| item["address"] == "10.25.0.2")
        .expect("看到過的列存在");
    assert_eq!(entry["last_seen_source"], "arp");
    assert!(observed_of(entry));

    let history = ip_history(&state, subnet_id, "10.25.0.2").await;
    assert_eq!(history["observed"], true, "細節亦為已觀測");
    assert_eq!(history["presence"]["last_seen_source"], "arp");

    // 代理過期（last_report_at 超過 AGENT_STALE_SECS=900 秒）：回到未觀測，
    // 資料保留不清除
    sqlx::query("UPDATE agent SET last_report_at = ? WHERE id = ?")
        .bind(timestamp(Utc::now() - Duration::seconds(1_000)))
        .bind(AGENT_ID)
        .execute(&pool)
        .await
        .expect("調整代理回報時間");

    let page = list_ips(&state, subnet_id).await;
    assert!(
        page["items"]
            .as_array()
            .expect("items")
            .iter()
            .all(|item| !observed_of(item)),
        "代理過期：未觀測"
    );

    let history = ip_history(&state, subnet_id, "10.25.0.2").await;
    assert_eq!(history["observed"], false, "代理過期：詳情未觀測");
    assert_eq!(
        history["presence"]["last_seen_source"], "arp",
        "過期不清資料"
    );
}

#[tokio::test]
async fn loopback_source_takes_first_xff_segment() {
    let pool = test_pool().await;
    let state = test_state(&pool);
    let _ = create_subnet(&state, "10.41.0.0/24", "XFF 區").await;

    // 本機反向代理（nginx）之後：連線來源為 loopback、XFF 為真實客戶端；
    // 多段（客戶端, 近端代理）取第一段（見 spec §4）。
    let xff = "203.0.113.7, 10.0.0.1";

    // 心跳：代理來源取 XFF 第一段
    let (status, body) = send_from(
        &state,
        Method::POST,
        "/api/v1/agents/heartbeat",
        LOOPBACK,
        Some(xff),
        Some(json!({
            "instance_id": AGENT_ID,
            "name": "edge-agent",
            "version": "0.1.0",
            "subnet_cidr": "10.41.0.0/24",
        })),
        Some(AUTH_CODE),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "心跳應成功：{body}");

    let (status, page) = send(&state, Method::GET, "/api/v1/agents", None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        page["items"][0]["source_ip"], "203.0.113.7",
        "心跳來源取 XFF 第一段"
    );

    // 觀測回報：代理來源同樣取 XFF 第一段
    let (status, body) = send_from(
        &state,
        Method::POST,
        "/api/v1/agents/observations",
        LOOPBACK,
        Some(xff),
        Some(sweep_body(
            AGENT_ID,
            "10.41.0.0/24",
            &ago(1),
            &["10.41.0.1"],
            &[],
        )),
        Some(AUTH_CODE),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "觀測應成功：{body}");

    // 錯碼（同 loopback＋XFF）：被拒回報的 source_ip 亦取 XFF 第一段
    let (status, response) = send_from(
        &state,
        Method::POST,
        "/api/v1/agents/observations",
        LOOPBACK,
        Some(xff),
        Some(sweep_body(AGENT_ID, "10.41.0.0/24", &ago(2), &[], &[])),
        Some("wrong-code"),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "錯碼須拒收：{response}");

    let (status, page) = send(&state, Method::GET, "/api/v1/agents", None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        page["items"][0]["source_ip"], "203.0.113.7",
        "觀測回報後代理來源仍取 XFF 第一段"
    );

    let (status, page) = send(
        &state,
        Method::GET,
        "/api/v1/agents/auth-failures",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let failure = &page["items"][0];
    assert_eq!(
        failure["source_ip"], "203.0.113.7",
        "被拒回報來源取 XFF 第一段"
    );
    assert_eq!(failure["attempt_count"], 1);
}

#[tokio::test]
async fn loopback_without_xff_uses_connection_source() {
    let pool = test_pool().await;
    let state = test_state(&pool);
    let _ = create_subnet(&state, "10.42.0.0/24", "本機區").await;

    // 無反向代理標頭時退回連線來源（127.0.0.1）
    let (status, body) = send_from(
        &state,
        Method::POST,
        "/api/v1/agents/heartbeat",
        LOOPBACK,
        None,
        Some(json!({
            "instance_id": AGENT_ID,
            "name": "edge-agent",
            "version": "0.1.0",
            "subnet_cidr": "10.42.0.0/24",
        })),
        Some(AUTH_CODE),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "心跳應成功：{body}");

    let (status, page) = send(&state, Method::GET, "/api/v1/agents", None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        page["items"][0]["source_ip"], "127.0.0.1",
        "loopback 無 XFF 用連線來源"
    );

    // 觀測回報亦然
    let (status, body) = send_from(
        &state,
        Method::POST,
        "/api/v1/agents/observations",
        LOOPBACK,
        None,
        Some(sweep_body(
            AGENT_ID,
            "10.42.0.0/24",
            &ago(1),
            &["10.42.0.1"],
            &[],
        )),
        Some(AUTH_CODE),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "觀測應成功：{body}");

    let (status, page) = send(&state, Method::GET, "/api/v1/agents", None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        page["items"][0]["source_ip"], "127.0.0.1",
        "觀測後來源仍為連線來源"
    );
}

#[tokio::test]
async fn non_loopback_ignores_forged_forwarded_for() {
    let pool = test_pool().await;
    let state = test_state(&pool);
    let _ = create_subnet(&state, "10.43.0.0/24", "偽造區").await;

    // 直接連線（非 loopback）帶偽造 XFF：不得採信，仍用連線來源
    let (status, body) = send_from(
        &state,
        Method::POST,
        "/api/v1/agents/heartbeat",
        REMOTE,
        Some("203.0.113.7, 10.0.0.1"),
        Some(json!({
            "instance_id": AGENT_ID,
            "name": "edge-agent",
            "version": "0.1.0",
            "subnet_cidr": "10.43.0.0/24",
        })),
        Some(AUTH_CODE),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "心跳應成功：{body}");

    let (status, page) = send(&state, Method::GET, "/api/v1/agents", None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        page["items"][0]["source_ip"], "203.0.113.9",
        "非 loopback 偽造 XFF 不採信"
    );

    // 錯碼回報亦同
    let (status, response) = send_from(
        &state,
        Method::POST,
        "/api/v1/agents/observations",
        REMOTE,
        Some("203.0.113.7"),
        Some(sweep_body(AGENT_ID, "10.43.0.0/24", &ago(1), &[], &[])),
        Some("wrong-code"),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "錯碼須拒收：{response}");

    let (status, page) = send(
        &state,
        Method::GET,
        "/api/v1/agents/auth-failures",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        page["items"][0]["source_ip"], "203.0.113.9",
        "被拒回報亦用連線來源"
    );
}
