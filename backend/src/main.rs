//! asset-nest 後端啟動點。

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Context;
use asset_nest::probe::SystemProber;
use asset_nest::{AppState, config::Config, db, kea, observation};
use axum::serve;
use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

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

    let mut state = AppState::new(pool, config.web_dist_dir.clone())
        .with_prober(Arc::new(SystemProber::with_mode(
            config.observation_probe_mode,
        )))
        .with_discovery_rate_pps(config.observation_discovery_rate_pps);
    if let Some(url) = config.kea_api_url.clone() {
        let mut client = kea::http::Client::new(url);
        if let Some(username) = config.kea_api_username.clone() {
            let password = config.kea_api_password.clone().unwrap_or_default();
            client = client.with_basic_auth(username, password);
        }
        state = state.with_kea(client);
    }

    // 背景排程：快速掃描（每網段 15 分鐘）／探索掃描（網段或全站間隔）
    // 與事件保留清理（啟動＋每日；見票 04、票 05）
    let scheduler = observation::spawn_scheduler(
        state.clone(),
        config.observation_retention_days,
        config.observation_discovery_interval_secs,
    );

    // 帶入連線來源資訊，供 `/api/v1/peer-mac` 反查 ARP（見票 09）
    let served = serve(
        listener,
        asset_nest::app(state).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .context("伺服器執行失敗");

    scheduler.abort();
    served?;

    Ok(())
}
