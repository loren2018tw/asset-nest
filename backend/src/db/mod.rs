//! 資料庫連線與遷移（見 `docs/adr/0003`）。

use std::str::FromStr;

use anyhow::Context;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::SqlitePool;

/// 建立連線池並套用內嵌 migrations。
pub async fn init(database_url: &str) -> anyhow::Result<SqlitePool> {
    let options = SqliteConnectOptions::from_str(database_url)
        .context("DATABASE_URL 格式錯誤")?
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal);

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await
        .context("無法連線資料庫")?;

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .context("套用 migrations 失敗")?;

    Ok(pool)
}
