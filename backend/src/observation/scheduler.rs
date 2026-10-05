//! 觀測背景排程器：快速掃描、探索掃描與保留清理的薄迴圈（見票 04、票 05、
//! spec §掃描服務）。
//!
//! 單一 tokio task、每 60 秒 tick 一次；迴圈內**依序**執行，同一時間最多一個
//! 掃描在跑，tick 不重疊。每輪：
//!
//! 1. 保留清理：啟動即執行一次，之後每 24 小時一次（`cleanup_events`）。
//! 2. 快速掃描：只掃「已開觀測 ∧ v4 ∧ 本機同 L2」的網段；每網段固定 15 分鐘
//!    一次（常數，不入設定）；啟動後無上次紀錄者視為 due。
//! 3. 探索掃描：同上再要求「已開探索」；間隔取網段 `discovery_interval_minutes`
//!    （分鐘）或全站 `OBSERVATION_DISCOVERY_INTERVAL_SECS`（秒）；啟動後無上次
//!    紀錄者視為 due。
//!
//! 掃描或清理失敗只記 `warn`，不中斷迴圈。due 判斷以注入的 `now` 計算，
//! 邏輯抽為純函式 [`due`] 供單元測試；掃描時 `now` 由 `Utc::now()` 帶入。

use std::collections::HashMap;
use std::time::Duration;

use chrono::{DateTime, TimeDelta, Utc};
use tokio::task::JoinHandle;

use crate::AppState;
use crate::observation::{cleanup_events, run_discovery, run_quick};
use crate::subnets::{self, Subnet};

/// 排程 tick 間隔（常數；見 spec §掃描服務）。
const TICK_INTERVAL: Duration = Duration::from_secs(60);
/// 快速掃描間隔：固定 15 分鐘／網段（常數，不入設定；見 spec §環境設定）。
const QUICK_INTERVAL: TimeDelta = TimeDelta::minutes(15);
/// 保留清理間隔：每日一次（清理範圍由 `OBSERVATION_RETENTION_DAYS` 決定）。
const RETENTION_INTERVAL: TimeDelta = TimeDelta::hours(24);

/// 是否到期待跑：從未跑過（`None`）視為 due；否則距上次已達 `interval`
/// （`last_run + interval <= now`，恰滿即 due）。
fn due(last_run: Option<DateTime<Utc>>, now: DateTime<Utc>, interval: TimeDelta) -> bool {
    match last_run {
        None => true,
        Some(last) => now >= last + interval,
    }
}

/// 探索掃描間隔：網段覆寫（分鐘、正整數）優先，否則用全站預設秒數。
///
/// 防禦性：非正覆寫值（理論上被 PATCH 驗證擋住）一律回退全站預設，
/// 避免 `0` 造成每 tick 熱迴圈。
fn discovery_interval(subnet: &Subnet, default_secs: u64) -> TimeDelta {
    match subnet.discovery_interval_minutes {
        Some(minutes) if minutes > 0 => TimeDelta::minutes(minutes),
        _ => {
            let capped = default_secs.min(i64::MAX as u64) as i64;
            TimeDelta::seconds(capped)
        }
    }
}

/// 啟動背景排程器；回傳 task handle 供 `main` 持有／中止。
///
/// 以 `state.kea.as_ref()`、`state.discovery_rate_pps` 與 `Utc::now()` 呼叫
/// [`run_quick`]／[`run_discovery`]，與手動掃描同一服務路徑；清理沿用
/// `retention_days`（`OBSERVATION_RETENTION_DAYS`）；探索全站預設間隔為
/// `discovery_interval_secs`（`OBSERVATION_DISCOVERY_INTERVAL_SECS`）。
pub fn spawn_scheduler(
    state: AppState,
    retention_days: u32,
    discovery_interval_secs: u64,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(TICK_INTERVAL);
        // 掃描可能跨過一個 tick；不追趕補跑，避免恢復時瞬間連續掃描。
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        let mut last_quick: HashMap<i64, DateTime<Utc>> = HashMap::new();
        let mut last_discovery: HashMap<i64, DateTime<Utc>> = HashMap::new();
        let mut last_cleanup: Option<DateTime<Utc>> = None;

        loop {
            ticker.tick().await;
            let now = Utc::now();

            // 保留清理（啟動即一次、之後每 24 小時）；與掃描同 task、不重疊。
            if due(last_cleanup, now, RETENTION_INTERVAL) {
                match cleanup_events(&state.db, retention_days, now).await {
                    Ok(deleted) => {
                        last_cleanup = Some(now);
                        tracing::info!(deleted, retention_days, "觀測事件保留清理完成");
                    }
                    Err(error) => {
                        tracing::warn!(error = %error.message(), "觀測事件保留清理失敗");
                    }
                }
            }

            // 每輪重新讀取網段清單：開關與本機判定即時反映，無需重啟。
            let subnets = match subnets::list_full(&state.db).await {
                Ok(subnets) => subnets,
                Err(error) => {
                    tracing::warn!(%error, "讀取網段清單失敗，本輪略過掃描");
                    continue;
                }
            };

            for subnet in subnets {
                if !subnet.observed || subnet.cidr.contains(':') || !state.prober.is_local(&subnet)
                {
                    continue;
                }

                if due(last_quick.get(&subnet.id).copied(), now, QUICK_INTERVAL) {
                    match run_quick(
                        &state.db,
                        state.prober.clone(),
                        state.kea.as_ref(),
                        &subnet,
                        now,
                    )
                    .await
                    {
                        Ok(report) => {
                            last_quick.insert(subnet.id, now);
                            tracing::info!(
                                subnet_id = subnet.id,
                                targets = report.targets,
                                seen = report.seen,
                                duration_ms = report.duration_ms,
                                "快速掃描完成"
                            );
                        }
                        Err(error) => {
                            // 失敗也記時間：下一週期（15 分鐘）再試，不每 tick 重打。
                            last_quick.insert(subnet.id, now);
                            tracing::warn!(
                                subnet_id = subnet.id,
                                error = %error.message(),
                                "快速掃描失敗"
                            );
                        }
                    }
                }

                // 探索掃描：同輪先快速後探索，依序執行即不重疊（見票 05）。
                if !subnet.discovery_enabled {
                    continue;
                }
                let interval = discovery_interval(&subnet, discovery_interval_secs);
                if !due(last_discovery.get(&subnet.id).copied(), now, interval) {
                    continue;
                }

                match run_discovery(
                    &state.db,
                    state.prober.clone(),
                    state.kea.as_ref(),
                    &subnet,
                    state.discovery_rate_pps,
                    now,
                )
                .await
                {
                    Ok(report) => {
                        last_discovery.insert(subnet.id, now);
                        tracing::info!(
                            subnet_id = subnet.id,
                            targets = report.targets,
                            seen = report.seen,
                            duration_ms = report.duration_ms,
                            "探索掃描完成"
                        );
                    }
                    Err(error) => {
                        // 失敗也記時間：下一週期再試，不每 tick 重打。
                        last_discovery.insert(subnet.id, now);
                        tracing::warn!(
                            subnet_id = subnet.id,
                            error = %error.message(),
                            "探索掃描失敗"
                        );
                    }
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 固定時間。
    fn at(text: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(text)
            .expect("固定時間")
            .with_timezone(&Utc)
    }

    #[test]
    fn due_covers_never_run_interval_boundary_and_overdue() {
        let now = at("2026-10-06T12:00:00Z");

        assert!(due(None, now, QUICK_INTERVAL), "從未執行視為 due");
        assert!(
            !due(Some(now - TimeDelta::minutes(14)), now, QUICK_INTERVAL),
            "未滿 15 分鐘不 due"
        );
        assert!(
            due(Some(now - TimeDelta::minutes(15)), now, QUICK_INTERVAL),
            "恰滿 15 分鐘即 due"
        );
        assert!(
            due(Some(now - TimeDelta::minutes(16)), now, QUICK_INTERVAL),
            "超過 15 分鐘 due"
        );
    }

    #[test]
    fn due_reuses_same_rule_for_daily_retention() {
        let now = at("2026-10-06T12:00:00Z");

        assert!(due(None, now, RETENTION_INTERVAL), "啟動即清理一次");
        assert!(!due(
            Some(now - TimeDelta::hours(23)),
            now,
            RETENTION_INTERVAL
        ));
        assert!(due(
            Some(now - TimeDelta::hours(24)),
            now,
            RETENTION_INTERVAL
        ));
    }

    /// 測試用網段：只在乎探索覆寫欄位，其餘為最小值。
    fn subnet(discovery_interval_minutes: Option<i64>) -> Subnet {
        Subnet {
            id: 1,
            cidr: "10.0.0.0/29".to_string(),
            name: None,
            note: None,
            gateway: None,
            kea_subnet_id: None,
            observed: true,
            discovery_enabled: true,
            discovery_interval_minutes,
            last_discovery_at: None,
            pools: Vec::new(),
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    #[test]
    fn discovery_interval_prefers_positive_subnet_override() {
        assert_eq!(
            discovery_interval(&subnet(Some(120)), 86_400),
            TimeDelta::minutes(120),
            "網段覆寫以分鐘計，優先於全站預設"
        );
        assert_eq!(
            discovery_interval(&subnet(None), 3_600),
            TimeDelta::seconds(3_600),
            "未覆寫時用全站預設秒數"
        );
        assert_eq!(
            discovery_interval(&subnet(Some(0)), 60),
            TimeDelta::seconds(60),
            "防禦性：非正覆寫回退全站預設，避免熱迴圈"
        );
        assert_eq!(
            discovery_interval(&subnet(Some(-5)), 60),
            TimeDelta::seconds(60)
        );
    }

    #[test]
    fn discovery_due_uses_override_and_never_run_semantics() {
        let now = at("2026-10-06T12:00:00Z");
        let interval = discovery_interval(&subnet(Some(30)), 86_400);

        assert!(due(None, now, interval), "從未探索視為 due");
        assert!(
            !due(Some(now - TimeDelta::minutes(29)), now, interval),
            "未滿網段覆寫的 30 分鐘不 due"
        );
        assert!(
            due(Some(now - TimeDelta::minutes(30)), now, interval),
            "恰滿即 due"
        );
    }
}
