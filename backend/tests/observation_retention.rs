//! 觀測事件保留清理整合測試（見票 04、ADR-0016）。
//!
//! 直接呼叫 [`cleanup_events`] 對套用 migrations 的記憶體 SQLite 驗證：
//! 只刪過期事件（含嚴格早於 cutoff 的界線）、現況列不受影響、清理為全站。

use chrono::{DateTime, Utc};
use sqlx::SqlitePool;
use sqlx::sqlite::SqlitePoolOptions;

use asset_nest::observation::cleanup_events;

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

/// 固定「現在」；cutoff ＝ 2025-10-06T00:00:00Z（365 天前）。
fn now() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-10-06T00:00:00Z")
        .expect("固定時間")
        .with_timezone(&Utc)
}

/// 植入測試網段，回傳 id。
async fn insert_subnet(pool: &SqlitePool, cidr: &str) -> i64 {
    sqlx::query("INSERT INTO subnets (cidr, observed) VALUES (?, 1)")
        .bind(cidr)
        .execute(pool)
        .await
        .expect("植入網段")
        .last_insert_rowid()
}

/// 植入一筆事件。
async fn insert_event(pool: &SqlitePool, subnet_id: i64, address: &str, observed_at: &str) {
    sqlx::query(
        "INSERT INTO observation_event (subnet_id, address, mac, kind, source, observed_at)
         VALUES (?, ?, 'aa:bb:cc:dd:ee:ff', 'first_seen', 'arp', ?)",
    )
    .bind(subnet_id)
    .bind(address)
    .bind(observed_at)
    .execute(pool)
    .await
    .expect("植入事件");
}

/// 植入現況列（`last_seen_at` 可為從未上線的 NULL）。
async fn insert_presence(
    pool: &SqlitePool,
    subnet_id: i64,
    address: &str,
    last_seen_at: Option<&str>,
) {
    sqlx::query(
        "INSERT INTO ip_presence
             (subnet_id, address, last_seen_at, last_seen_mac, last_seen_source, last_checked_at)
         VALUES (?, ?, ?, ?, 'arp', '2026-10-06T00:00:00Z')",
    )
    .bind(subnet_id)
    .bind(address)
    .bind(last_seen_at)
    .bind(if last_seen_at.is_some() {
        Some("aa:bb:cc:dd:ee:ff")
    } else {
        None
    })
    .execute(pool)
    .await
    .expect("植入現況");
}

/// 剩餘事件（網段 id、位址、時間），依寫入順序。
async fn events(pool: &SqlitePool) -> Vec<(i64, String, String)> {
    sqlx::query_as("SELECT subnet_id, address, observed_at FROM observation_event ORDER BY id ASC")
        .fetch_all(pool)
        .await
        .expect("讀取事件")
}

#[tokio::test]
async fn cleanup_deletes_only_expired_events_and_keeps_presence_intact() {
    let pool = test_pool().await;
    let first = insert_subnet(&pool, "10.0.0.0/29").await;
    let second = insert_subnet(&pool, "10.0.1.0/29").await;

    // 第一網段：一筆過期、一筆恰在 cutoff（保留）、一筆新鮮；兩列現況。
    insert_event(&pool, first, "10.0.0.1", "2025-01-01T00:00:00Z").await;
    insert_event(&pool, first, "10.0.0.2", "2025-10-06T00:00:00Z").await;
    insert_event(&pool, first, "10.0.0.3", "2026-10-05T23:59:59Z").await;
    insert_presence(&pool, first, "10.0.0.1", Some("2026-10-05T00:00:00Z")).await;
    insert_presence(&pool, first, "10.0.0.4", None).await;

    // 第二網段：清理為全站，過期事件同樣刪除、新鮮保留。
    insert_event(&pool, second, "10.0.1.1", "2024-06-01T00:00:00Z").await;
    insert_event(&pool, second, "10.0.1.1", "2026-01-01T00:00:00Z").await;

    let deleted = cleanup_events(&pool, 365, now()).await.expect("清理成功");

    assert_eq!(deleted, 2, "兩網段各刪一筆嚴格早於 cutoff 的事件");
    assert_eq!(
        events(&pool).await,
        vec![
            (
                first,
                "10.0.0.2".to_string(),
                "2025-10-06T00:00:00Z".to_string()
            ),
            (
                first,
                "10.0.0.3".to_string(),
                "2026-10-05T23:59:59Z".to_string()
            ),
            (
                second,
                "10.0.1.1".to_string(),
                "2026-01-01T00:00:00Z".to_string()
            ),
        ],
        "恰在 cutoff 的事件保留；跨網段只刪過期者"
    );

    // 現況完整保留：last_seen 原值不動、「從未上線」仍為 NULL、檢查時間不動。
    let presence: Vec<(String, Option<String>, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT address, last_seen_at, last_seen_mac, last_checked_at
           FROM ip_presence WHERE subnet_id = ? ORDER BY address",
    )
    .bind(first)
    .fetch_all(&pool)
    .await
    .expect("讀取現況");

    assert_eq!(
        presence,
        vec![
            (
                "10.0.0.1".to_string(),
                Some("2026-10-05T00:00:00Z".to_string()),
                Some("aa:bb:cc:dd:ee:ff".to_string()),
                Some("2026-10-06T00:00:00Z".to_string()),
            ),
            (
                "10.0.0.4".to_string(),
                None,
                None,
                Some("2026-10-06T00:00:00Z".to_string()),
            ),
        ],
        "清理只動事件、不動 ip_presence"
    );
}

#[tokio::test]
async fn cleanup_is_a_noop_when_nothing_is_expired() {
    let pool = test_pool().await;
    let subnet = insert_subnet(&pool, "10.0.0.0/29").await;
    insert_event(&pool, subnet, "10.0.0.1", "2026-01-01T00:00:00Z").await;
    insert_presence(&pool, subnet, "10.0.0.1", Some("2026-01-01T00:00:00Z")).await;

    let deleted = cleanup_events(&pool, 365, now()).await.expect("清理成功");

    assert_eq!(deleted, 0, "無過期事件時刪除 0 筆");
    assert_eq!(events(&pool).await.len(), 1, "事件保留");
    let presence_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM ip_presence")
        .fetch_one(&pool)
        .await
        .expect("讀取現況筆數");
    assert_eq!(presence_count, 1, "現況保留");
}
