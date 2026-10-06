//! asset-nest 後端啟動點。

use std::net::SocketAddr;
use std::time::Duration;

use anyhow::Context;
use asset_nest::{AppState, agents, config::Config, db, kea, observation};
use axum::serve;
use chrono::Utc;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

/// Kea 租約觀測週期（比照舊快速掃描的 15 分鐘；見票 08）。
const LEASE_INTERVAL: Duration = Duration::from_secs(15 * 60);
/// 保留清理週期：每日一次（事件＋被拒回報；見票 08）。
const CLEANUP_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

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
        .with_agent_auth_code(config.agent_auth_code.clone())
        .with_agent_stale_secs(config.agent_stale_secs);
    if let Some(url) = config.kea_api_url.clone() {
        let mut client = kea::http::Client::new(url);
        if let Some(username) = config.kea_api_username.clone() {
            let password = config.kea_api_password.clone().unwrap_or_default();
            client = client.with_basic_auth(username, password);
        }
        state = state.with_kea(client);
    }

    // 背景維護：Kea 租約觀測（每 15 分鐘）與保留清理（啟動＋每日；
    // 事件＋被拒回報）。探測執行已外移給觀測代理（見 ADR-0018、票 08）。
    let maintenance = spawn_maintenance(
        state.clone(),
        config.observation_retention_days,
        config.agent_auth_failure_retention_days,
    );

    // 帶入連線來源資訊，供 `/api/v1/peer-mac` 反查 ARP（見票 09）
    let served = serve(
        listener,
        asset_nest::app(state).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .context("伺服器執行失敗");

    maintenance.abort();
    served?;

    Ok(())
}

/// 背景維護薄迴圈：Kea 租約觀測與保留清理，無掃描職責。
///
/// 兩個週期各自獨立（select 不重疊執行）；失敗只記警告、不中斷迴圈。
/// 清理沿用 [`observation::cleanup_events`] 與 [`agents::cleanup_auth_failures`]，
/// 保留天數分別來自 `OBSERVATION_RETENTION_DAYS` 與
/// `AGENT_AUTH_FAILURE_RETENTION_DAYS`。
fn spawn_maintenance(
    state: AppState,
    retention_days: u32,
    auth_failure_retention_days: u32,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut lease_ticker = tokio::time::interval(LEASE_INTERVAL);
        lease_ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut cleanup_ticker = tokio::time::interval(CLEANUP_INTERVAL);
        cleanup_ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        loop {
            tokio::select! {
                _ = lease_ticker.tick() => {
                    match observation::record_lease_observations(
                        &state.db,
                        state.kea.as_ref(),
                    )
                    .await
                    {
                        Ok(recorded) => {
                            tracing::info!(recorded, "Kea 租約觀測完成");
                        }
                        Err(error) => {
                            tracing::warn!(?error, "Kea 租約觀測失敗");
                        }
                    }
                }
                _ = cleanup_ticker.tick() => {
                    let now = Utc::now();
                    match observation::cleanup_events(&state.db, retention_days, now).await {
                        Ok(deleted) => {
                            tracing::info!(deleted, retention_days, "觀測事件保留清理完成");
                        }
                        Err(error) => {
                            tracing::warn!(?error, "觀測事件保留清理失敗");
                        }
                    }
                    match agents::cleanup_auth_failures(
                        &state.db,
                        auth_failure_retention_days,
                        now,
                    )
                    .await
                    {
                        Ok(deleted) => {
                            tracing::info!(
                                deleted,
                                auth_failure_retention_days,
                                "代理被拒回報保留清理完成"
                            );
                        }
                        Err(error) => {
                            tracing::warn!(?error, "代理被拒回報保留清理失敗");
                        }
                    }
                }
            }
        }
    })
}
