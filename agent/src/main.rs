//! asset-nest 觀測代理啟動點。

use anyhow::Context;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let config = asset_nest_agent::config::Config::from_env().context("讀取環境設定失敗")?;
    tracing::info!(
        server = %config.server_url,
        subnet = %config.subnet,
        name = %config.name,
        sweep_interval_secs = config.sweep_interval.as_secs(),
        sweep_rate_pps = config.sweep_rate_pps,
        "觀測代理啟動"
    );

    asset_nest_agent::runner::run(config).await
}
