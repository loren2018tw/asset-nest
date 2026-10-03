//! asset-nest 後端啟動點。

use anyhow::Context;
use asset_nest::{config::Config, db, AppState};
use axum::serve;
use tokio::net::TcpListener;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,tower_http=info".into()))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let config = Config::from_env().context("讀取環境設定失敗")?;
    tracing::debug!(?config, "環境設定");

    let pool = db::init(&config.database_url)
        .await
        .context("初始化資料庫失敗")?;

    if config.web_dist_dir.join("index.html").exists() {
        tracing::info!(dir = %config.web_dist_dir.display(), "提供前端靜態檔");
    } else {
        tracing::warn!(
            dir = %config.web_dist_dir.display(),
            "找不到前端建置產物；開發時請使用 Quasar dev server（pnpm dev:frontend）"
        );
    }

    let listener = TcpListener::bind(config.bind_addr)
        .await
        .with_context(|| format!("無法綁定 {}", config.bind_addr))?;
    tracing::info!("asset-nest 後端：http://{}", listener.local_addr()?);

    let state = AppState::new(pool, config.web_dist_dir.clone());
    serve(listener, asset_nest::app(state))
        .await
        .context("伺服器執行失敗")?;

    Ok(())
}
