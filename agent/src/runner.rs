//! 代理組裝：掃描、被動監聽、推送與心跳四個背景任務（見 spec §行為）。
//!
//! - 掃描：每 `AGENT_SWEEP_INTERVAL_SECS` 一次；**首次在間隔後**（啟動當下
//!   不掃描，CI 以大間隔即可完全不掃描）。
//! - 被動：raw 監聽常駐（阻塞執行緒；只收不送），每 30 秒 flush 有更新者；
//!   與掃描並行、互不阻塞。
//! - 推送：佇列空時等待；失敗依 [`Backoff`] 退避（5s→5min）。
//! - 心跳：啟動後立即一次，之後每 60 秒；`subnet_matched:false` 只在狀態
//!   變化時警告。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::Context;
use chrono::Utc;
use ipnet::Ipv4Net;
use tokio::time::MissedTickBehavior;

use crate::VERSION;
use crate::config::Config;
use crate::heartbeat::{HEARTBEAT_INTERVAL, Heartbeat, MatchChange, MatchTracker};
use crate::passive::{self, Aggregator};
use crate::probe;
use crate::push::{
    self, AgentInfo, Backoff, FlushResult, PushQueue, Pusher, QUEUE_CAPACITY, Seen, SweepReport,
};

/// 代理主流程：建立推送器與佇列、啟動背景任務，直到收到 Ctrl-C。
pub async fn run(config: Config) -> anyhow::Result<()> {
    let agent = AgentInfo {
        instance_id: config.instance_id.clone(),
        name: config.name.clone(),
        version: VERSION.to_string(),
        subnet_cidr: config.subnet.to_string(),
    };

    let client = reqwest::Client::builder()
        .build()
        .context("建立 HTTP 客戶端失敗")?;
    let pusher = Pusher::new(
        client.clone(),
        &config.server_url,
        &config.auth_code,
        agent.clone(),
    )?;
    let heartbeat = Heartbeat::new(client, &config.server_url, &config.auth_code, agent)?;
    let queue = Arc::new(PushQueue::new(QUEUE_CAPACITY));
    let shutdown = Arc::new(AtomicBool::new(false));
    let aggregator = Arc::new(Mutex::new(Aggregator::new()));

    // 能力檢查不阻擋啟動；缺 CAP_NET_RAW 等部署問題在此先提示。
    if let Err(error) = probe::raw_available(&config.subnet) {
        tracing::warn!(
            %error,
            "raw ARP 目前不可用；主動掃描將無法送出（需 CAP_NET_RAW 與網段內介面）"
        );
    }

    let sweep_task = tokio::spawn(sweep_loop(config.clone(), Arc::clone(&queue)));
    let passive_listener_task = spawn_passive_listener(
        config.subnet,
        Arc::clone(&shutdown),
        Arc::clone(&aggregator),
    );
    let passive_flush_task = tokio::spawn(passive_loop(Arc::clone(&queue), aggregator));
    let push_task = tokio::spawn(push_loop(pusher, Arc::clone(&queue)));
    let heartbeat_task = tokio::spawn(heartbeat_loop(heartbeat));

    tracing::info!(
        subnet = %config.subnet,
        sweep_interval_secs = config.sweep_interval.as_secs(),
        "代理常駐啟動（心跳 60 秒、首次掃描在間隔後、被動監聽持續並每 30 秒 flush）"
    );

    tokio::signal::ctrl_c().await.context("等待結束訊號失敗")?;
    tracing::info!("收到結束訊號，停止代理");
    shutdown.store(true, Ordering::Relaxed);
    sweep_task.abort();
    passive_flush_task.abort();
    push_task.abort();
    heartbeat_task.abort();
    // 監聽在阻塞執行緒中輪詢停止旗標（單次 recv 等待上限 200ms），等待其收尾。
    let _ = passive_listener_task.await;
    Ok(())
}

/// 週期主動掃描：首次在間隔後執行；結果切批入列並喚醒推送。
async fn sweep_loop(config: Config, queue: Arc<PushQueue>) {
    let mut ticker = tokio::time::interval_at(
        tokio::time::Instant::now() + config.sweep_interval,
        config.sweep_interval,
    );
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

    loop {
        ticker.tick().await;

        let observed_at = Utc::now();
        match probe::sweep(config.subnet, config.sweep_rate_pps).await {
            Ok(result) => {
                let seen_count = result.seen.len();
                let report = SweepReport::new(
                    observed_at,
                    result.checked,
                    result
                        .seen
                        .into_iter()
                        .map(|(address, mac)| Seen { address, mac })
                        .collect(),
                );
                let entries = report.entries();
                let dropped = queue.push(report);
                tracing::info!(
                    entries,
                    seen = seen_count,
                    dropped,
                    "掃描完成並排入推送佇列"
                );
                queue.notify();
            }
            Err(error) => tracing::warn!(%error, "主動掃描失敗，本輪略過"),
        }
    }
}

/// 啟動持續被動監聽（阻塞；見 spec §持續被動）：合法 sender 寫入聚合器。
///
/// 監聽不可用時記一次警告後結束（被動觀測缺席；掃描與推送照常）。
fn spawn_passive_listener(
    network: Ipv4Net,
    shutdown: Arc<AtomicBool>,
    aggregator: Arc<Mutex<Aggregator>>,
) -> tokio::task::JoinHandle<()> {
    tokio::task::spawn_blocking(move || {
        let result = probe::passive_listen(
            &network,
            || shutdown.load(Ordering::Relaxed),
            |address, mac| {
                aggregator.lock().expect("被動聚合鎖").observe(address, mac);
            },
        );

        match result {
            Ok(()) => tracing::debug!("被動監聽已停止"),
            Err(error) => tracing::warn!(
                %error,
                "被動監聽不可用；將不產生被動觀測（需 CAP_NET_RAW 與網段內介面）"
            ),
        }
    })
}

/// 被動 flush 迴圈：首次在間隔後，每 30 秒將有更新者排入推送佇列；
/// 無更新不送。
async fn passive_loop(queue: Arc<PushQueue>, aggregator: Arc<Mutex<Aggregator>>) {
    let mut ticker = tokio::time::interval_at(
        tokio::time::Instant::now() + passive::FLUSH_INTERVAL,
        passive::FLUSH_INTERVAL,
    );
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

    loop {
        ticker.tick().await;

        let report = aggregator.lock().expect("被動聚合鎖").flush(Utc::now());
        let Some(report) = report else {
            continue; // 無更新不送
        };

        let entries = report.entries();
        let dropped = queue.push(report);
        tracing::info!(entries, dropped, "被動觀測已排入推送佇列");
        queue.notify();
    }
}

/// 推送迴圈：成功或丟棄後立即續送；失敗依退避等待後重試。
async fn push_loop(pusher: Pusher, queue: Arc<PushQueue>) {
    let mut backoff = Backoff::default();

    loop {
        match push::try_flush(&pusher, &queue).await {
            FlushResult::Empty => queue.notified().await,
            FlushResult::Stored { entries } => {
                backoff.reset();
                tracing::info!(entries, "觀測回報已入庫");
            }
            FlushResult::Dropped { entries } => {
                backoff.reset();
                tracing::debug!(entries, "未對應批次已丟棄");
            }
            FlushResult::Unauthorized { entries } => {
                let delay = backoff.next_delay();
                tracing::warn!(
                    entries,
                    delay_secs = delay.as_secs(),
                    "401 認證碼不符，將退避後重試"
                );
                tokio::time::sleep(delay).await;
            }
            FlushResult::Retry { entries, reason } => {
                let delay = backoff.next_delay();
                tracing::warn!(
                    entries,
                    delay_secs = delay.as_secs(),
                    %reason,
                    "推送失敗，將退避後重試"
                );
                tokio::time::sleep(delay).await;
            }
        }
    }
}

/// 心跳迴圈：立即一次、之後每 60 秒；未對應只在狀態變化時警告。
async fn heartbeat_loop(heartbeat: Heartbeat) {
    let mut tracker = MatchTracker::default();
    let mut ticker = tokio::time::interval(HEARTBEAT_INTERVAL);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

    loop {
        ticker.tick().await;

        match heartbeat.send().await {
            Ok(matched) => match tracker.observe(matched) {
                MatchChange::First(false) | MatchChange::NowMismatched => tracing::warn!(
                    "後端回報 subnet_matched=false：此代理網段未對應受管網段（觀測將不入庫）"
                ),
                MatchChange::NowMatched => tracing::info!("後端回報代理網段已對應"),
                MatchChange::First(true) | MatchChange::Unchanged => {}
            },
            Err(error) => tracing::warn!(%error, "心跳失敗；將於下個週期重試"),
        }
    }
}
