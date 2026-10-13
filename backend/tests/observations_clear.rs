//! 清除觀測紀錄整合測試（見 `.scratch/clear-observations` spec、ADR-0014）。
//!
//! 覆蓋：`DELETE /api/v1/observations` 刪除全部現況（含網段外列）與事件、
//! 回傳筆數、宣告與代理與被拒回報不受影響、冪等、需登入（401）與
//! 帶 session token 可清除。記憶體 SQLite＋`sqlx::migrate!`＋
//! `tower::ServiceExt::oneshot`；連線來源以注入 `ConnectInfo` 模擬
//! （比照 `tests/auth.rs`）。

use std::net::SocketAddr;
use std::path::PathBuf;

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Method, Request, StatusCode, header};
use chrono::Utc;
use http_body_util::BodyExt;
use serde_json::Value;
use sqlx::SqlitePool;
use sqlx::sqlite::SqlitePoolOptions;
use tower::ServiceExt;

use asset_nest::auth::{AuthConfig, SESSION_COOKIE_NAME, sign_token};
use asset_nest::{AppState, app};

/// 測試帳密（僅認證測試使用）。
const USERNAME: &str = "admin";
const PASSWORD: &str = "secret-clear";
/// 測試連線來源（比照 `tests/auth.rs`）。
const REMOTE: &str = "203.0.113.9:40123";

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

fn dist_dir() -> PathBuf {
    std::env::temp_dir().join("asset-nest-test-no-dist")
}

/// 測試帳密的認證設定（簽 token 用）。
fn auth_config() -> AuthConfig {
    AuthConfig {
        username: USERNAME.to_string(),
        password: PASSWORD.to_string(),
    }
}

/// 以可選 session token 發送請求；回傳狀態碼與 JSON。
async fn send(
    state: &AppState,
    method: Method,
    uri: &str,
    token: Option<&str>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(token) = token {
        builder = builder.header(header::COOKIE, format!("{SESSION_COOKIE_NAME}={token}"));
    }

    let mut request = builder.body(Body::empty()).expect("建立請求");
    request
        .extensions_mut()
        .insert(ConnectInfo(REMOTE.parse::<SocketAddr>().expect("來源位址")));

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

/// 讀取資料表列數（表名為測試內固定字串）。
async fn count(pool: &SqlitePool, table: &str) -> i64 {
    sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
        .fetch_one(pool)
        .await
        .expect("查詢列數")
}

/// 植入一組完整資料：網段、宣告（資產／介面／指派）、觀測（現況含網段外列、
/// 事件）、代理登錄與被拒回報各一筆。
async fn seed(pool: &SqlitePool) {
    let subnet_id =
        sqlx::query("INSERT INTO subnets (cidr, name) VALUES ('10.60.0.0/24', '清除測試區')")
            .execute(pool)
            .await
            .expect("植入網段")
            .last_insert_rowid();

    let asset_id =
        sqlx::query("INSERT INTO assets (description, location) VALUES ('測試資產', '機房')")
            .execute(pool)
            .await
            .expect("植入資產")
            .last_insert_rowid();

    let interface_id = sqlx::query(
        "INSERT INTO interfaces (asset_id, name, mac) VALUES (?, 'eth0', 'aa:bb:cc:dd:ee:60')",
    )
    .bind(asset_id)
    .execute(pool)
    .await
    .expect("植入介面")
    .last_insert_rowid();

    sqlx::query(
        "INSERT INTO ip_assignments (subnet_id, address, interface_id, purpose)
         VALUES (?, '10.60.0.10', ?, 'static')",
    )
    .bind(subnet_id)
    .bind(interface_id)
    .execute(pool)
    .await
    .expect("植入指派");

    // 現況三列：一列有 last_seen、一列只有 last_checked（從未上線）、
    // 一列網段外觀測。
    for (address, seen_at, mac) in [
        (
            "10.60.0.10",
            Some("2026-10-13T01:00:00Z"),
            Some("aa:bb:cc:dd:ee:60"),
        ),
        ("10.60.0.11", None, None),
    ] {
        sqlx::query(
            "INSERT INTO ip_presence
                 (subnet_id, address, last_seen_at, last_seen_mac, last_seen_source, last_checked_at)
             VALUES (?, ?, ?, ?, ?, '2026-10-13T01:00:00Z')",
        )
        .bind(subnet_id)
        .bind(address)
        .bind(seen_at)
        .bind(mac)
        .bind(seen_at.map(|_| "arp"))
        .execute(pool)
        .await
        .expect("植入現況");
    }
    sqlx::query(
        "INSERT INTO ip_presence
             (subnet_id, address, last_seen_at, last_seen_mac, last_seen_source, out_of_subnet)
         VALUES (?, '10.60.9.9', '2026-10-13T01:00:00Z', 'aa:bb:cc:dd:ee:99', 'arp_passive', 1)",
    )
    .bind(subnet_id)
    .execute(pool)
    .await
    .expect("植入網段外觀測列");

    // 事件兩筆。
    for (address, kind, observed_at) in [
        ("10.60.0.10", "first_seen", "2026-10-13T01:00:00Z"),
        ("10.60.0.10", "mac_changed", "2026-10-13T02:00:00Z"),
    ] {
        sqlx::query(
            "INSERT INTO observation_event (subnet_id, address, mac, kind, source, observed_at)
             VALUES (?, ?, 'aa:bb:cc:dd:ee:60', ?, 'arp', ?)",
        )
        .bind(subnet_id)
        .bind(address)
        .bind(kind)
        .bind(observed_at)
        .execute(pool)
        .await
        .expect("植入事件");
    }

    sqlx::query(
        "INSERT INTO agent
             (id, name, version, source_ip, subnet_cidr, subnet_id, first_report_at, last_report_at)
         VALUES ('agent-clear-01', 'clear-agent', '0.1.0', '203.0.113.9', '10.60.0.0/24', ?,
                 '2026-10-13T00:00:00Z', '2026-10-13T00:00:00Z')",
    )
    .bind(subnet_id)
    .execute(pool)
    .await
    .expect("植入代理");

    sqlx::query(
        "INSERT INTO agent_auth_failure
             (source_ip, claimed_name, claimed_version, first_attempt_at, last_attempt_at, attempt_count)
         VALUES ('203.0.113.50', 'ghost', '9.9.9', '2026-10-13T00:00:00Z', '2026-10-13T00:00:00Z', 3)",
    )
    .execute(pool)
    .await
    .expect("植入被拒回報");
}

/// 宣告與代理相關表在清除後應維持原狀（各一筆）。
async fn assert_non_observation_tables_untouched(pool: &SqlitePool) {
    for table in [
        "subnets",
        "assets",
        "interfaces",
        "ip_assignments",
        "agent",
        "agent_auth_failure",
    ] {
        assert_eq!(count(pool, table).await, 1, "{table} 不受清除影響");
    }
}

#[tokio::test]
async fn clear_deletes_all_observations_and_returns_counts() {
    let pool = test_pool().await;
    let state = AppState::new(pool.clone(), dist_dir());
    seed(&pool).await;

    let (status, json) = send(&state, Method::DELETE, "/api/v1/observations", None).await;
    assert_eq!(status, StatusCode::OK, "清除應成功：{json}");
    assert_eq!(json["presence_deleted"], 3, "現況含網段外列共三筆");
    assert_eq!(json["events_deleted"], 2, "事件兩筆");

    assert_eq!(count(&pool, "ip_presence").await, 0, "現況須清空");
    assert_eq!(count(&pool, "observation_event").await, 0, "事件須清空");
    assert_non_observation_tables_untouched(&pool).await;
}

#[tokio::test]
async fn clear_is_idempotent_and_empty_db_returns_zeros() {
    let pool = test_pool().await;
    let state = AppState::new(pool.clone(), dist_dir());

    // 空資料庫：首次呼叫即回零筆
    let (status, json) = send(&state, Method::DELETE, "/api/v1/observations", None).await;
    assert_eq!(status, StatusCode::OK, "{json}");
    assert_eq!(json["presence_deleted"], 0);
    assert_eq!(json["events_deleted"], 0);

    // 有資料清除後再呼叫一次：仍成功、回零筆
    seed(&pool).await;
    let (status, _) = send(&state, Method::DELETE, "/api/v1/observations", None).await;
    assert_eq!(status, StatusCode::OK);
    let (status, json) = send(&state, Method::DELETE, "/api/v1/observations", None).await;
    assert_eq!(status, StatusCode::OK, "重複清除應成功：{json}");
    assert_eq!(json["presence_deleted"], 0);
    assert_eq!(json["events_deleted"], 0);
}

#[tokio::test]
async fn clear_requires_login_when_auth_is_configured() {
    let pool = test_pool().await;
    let state = AppState::new(pool.clone(), dist_dir()).with_auth(USERNAME, PASSWORD);
    seed(&pool).await;

    // 未帶 cookie：401 且資料不動
    let (status, json) = send(&state, Method::DELETE, "/api/v1/observations", None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "未登入須拒收：{json}");
    assert_eq!(json["error"], "unauthorized");
    assert_eq!(count(&pool, "ip_presence").await, 3, "401 不得刪除現況");
    assert_eq!(
        count(&pool, "observation_event").await,
        2,
        "401 不得刪除事件"
    );

    // 有效 session token：200 並清除
    let token = sign_token(&auth_config(), Utc::now());
    let (status, json) = send(&state, Method::DELETE, "/api/v1/observations", Some(&token)).await;
    assert_eq!(status, StatusCode::OK, "登入後應可清除：{json}");
    assert_eq!(json["presence_deleted"], 3);
    assert_eq!(json["events_deleted"], 2);
    assert_eq!(count(&pool, "ip_presence").await, 0);
    assert_eq!(count(&pool, "observation_event").await, 0);
    assert_non_observation_tables_untouched(&pool).await;
}
