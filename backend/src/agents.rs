//! 觀測代理（observation agent）資料存取：心跳 upsert、觀測入庫（sweep／
//! passive；單一交易）與被拒回報、清單讀取。
//!
//! 詞彙依 `GLOSSARY.md`「觀測詞彙」；信任模型見 `docs/adr/0019`；網段外被動
//! 觀測規則見 `docs/adr/0017`（只記代理網段 CIDR 外的 sender）。

use std::net::{IpAddr, Ipv4Addr};

use chrono::{DateTime, Duration, Utc};
use ipnet::IpNet;
use serde::Serialize;
use sqlx::{FromRow, SqliteConnection, SqlitePool};

use crate::api::ApiError;
use crate::observation::{record_passive_seen, record_seen, timestamp, upsert_checked};

/// `GET /api/v1/agents` 的一列（見票 01、spec §HTTP API）。
#[derive(Debug, Serialize)]
pub struct Agent {
    /// 代理 `instance_id`（`agent.id`）。
    pub instance_id: String,
    pub name: String,
    pub version: String,
    pub source_ip: String,
    pub subnet_cidr: String,
    /// 精確對應的受管網段；對不到（或網段已刪除）為 `None`。
    pub subnet_id: Option<i64>,
    pub subnet_name: Option<String>,
    pub first_report_at: String,
    pub last_report_at: String,
    /// 最後一次成功入庫的觀測回報時間；心跳不算（見票 02）。
    pub last_observation_at: Option<String>,
    /// `last_report_at` 距 `now` 不超過 `stale_secs`。
    pub online: bool,
}

/// [`list`] 的查詢列。
#[derive(Debug, FromRow)]
struct AgentRow {
    instance_id: String,
    name: String,
    version: String,
    source_ip: String,
    subnet_cidr: String,
    subnet_id: Option<i64>,
    subnet_name: Option<String>,
    first_report_at: String,
    last_report_at: String,
    last_observation_at: Option<String>,
}

/// `GET /api/v1/agents/auth-failures` 的一列（以來源 IP 彙總；見 ADR-0019）。
#[derive(Debug, Serialize)]
pub struct AuthFailure {
    pub source_ip: String,
    pub claimed_name: Option<String>,
    pub claimed_version: Option<String>,
    pub first_attempt_at: String,
    pub last_attempt_at: String,
    pub attempt_count: i64,
}

/// [`list_auth_failures`] 的查詢列。
#[derive(Debug, FromRow)]
struct AuthFailureRow {
    source_ip: String,
    claimed_name: Option<String>,
    claimed_version: Option<String>,
    first_attempt_at: String,
    last_attempt_at: String,
    attempt_count: i64,
}

/// 已驗證的心跳輸入（`api::agents` 驗證後傳入）。
#[derive(Debug, Clone)]
pub struct Heartbeat {
    pub instance_id: String,
    pub name: String,
    pub version: String,
    /// 已正規化的 CIDR（呼叫端已將 host bits 收斂為網路地址）。
    pub subnet_cidr: IpNet,
}

/// 已驗證的單筆 sweep 報告（`api::agents` 驗證後傳入；見票 02）。
#[derive(Debug, Clone)]
pub struct SweepReport {
    /// 報告的觀測時間（UTC；寫入時未來值夾到「現在」）。
    pub observed_at: DateTime<Utc>,
    /// 已檢查的位址（全部更新最後檢查）。
    pub checked: Vec<Ipv4Addr>,
    /// 有回應的位址與正規化小寫冒號 MAC（更新最後可見並推導事件）。
    pub seen: Vec<(Ipv4Addr, String)>,
}

/// 已驗證的單筆 passive 報告（`api::agents` 驗證後傳入；見票 03）。
#[derive(Debug, Clone)]
pub struct PassiveReport {
    /// 報告的觀測時間（UTC；寫入時未來值夾到「現在」）。
    pub observed_at: DateTime<Utc>,
    /// 被動聽到的 sender 位址與正規化小寫冒號 MAC；CIDR 內者於入庫時丟棄。
    pub senders: Vec<(Ipv4Addr, String)>,
}

/// 已驗證的單筆觀測報告（見票 02、票 03）；混批時依 `observed_at` 排序、
/// 在同一交易內套用。
#[derive(Debug, Clone)]
pub enum ReportEntry {
    Sweep(SweepReport),
    Passive(PassiveReport),
}

impl ReportEntry {
    /// 報告的觀測時間（UTC；排序鍵）。
    pub fn observed_at(&self) -> DateTime<Utc> {
        match self {
            ReportEntry::Sweep(report) => report.observed_at,
            ReportEntry::Passive(report) => report.observed_at,
        }
    }
}

/// 已驗證的觀測回報（`api::agents` 驗證後傳入；見票 02、票 03）。
#[derive(Debug, Clone)]
pub struct ObservationReport {
    pub agent: Heartbeat,
    pub reports: Vec<ReportEntry>,
}

/// 以正規化 CIDR 精確比對受管網段（`subnets.cidr`）；對不到回 `None`。
pub async fn find_subnet_id(pool: &SqlitePool, cidr: &IpNet) -> Result<Option<i64>, ApiError> {
    sqlx::query_scalar("SELECT id FROM subnets WHERE cidr = ?")
        .bind(cidr.to_string())
        .fetch_optional(pool)
        .await
        .map_err(|error| ApiError::internal("讀取代理對應網段失敗", error))
}

/// upsert 代理（心跳與觀測回報共用）：`first_report_at` 只在首次寫入，
/// `last_report_at` 每回報更新（規格：心跳或回報皆算回報；見票 01、票 02）。
async fn upsert_agent(
    connection: &mut SqliteConnection,
    heartbeat: &Heartbeat,
    source_ip: &str,
    subnet_id: Option<i64>,
    at: &str,
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO agent
             (id, name, version, source_ip, subnet_cidr, subnet_id, first_report_at, last_report_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT (id) DO UPDATE SET
             name = excluded.name,
             version = excluded.version,
             source_ip = excluded.source_ip,
             subnet_cidr = excluded.subnet_cidr,
             subnet_id = excluded.subnet_id,
             last_report_at = excluded.last_report_at",
    )
    .bind(&heartbeat.instance_id)
    .bind(&heartbeat.name)
    .bind(&heartbeat.version)
    .bind(source_ip)
    .bind(heartbeat.subnet_cidr.to_string())
    .bind(subnet_id)
    .bind(at)
    .bind(at)
    .execute(&mut *connection)
    .await?;
    Ok(())
}

/// upsert 代理心跳：`first_report_at` 只在首次寫入，`last_report_at` 每回報更新。
pub async fn record_heartbeat(
    pool: &SqlitePool,
    heartbeat: &Heartbeat,
    source_ip: &str,
    subnet_id: Option<i64>,
    now: DateTime<Utc>,
) -> Result<(), ApiError> {
    let mut connection = pool
        .acquire()
        .await
        .map_err(|error| ApiError::internal("取得資料庫連線失敗", error))?;

    upsert_agent(
        &mut connection,
        heartbeat,
        source_ip,
        subnet_id,
        &timestamp(now),
    )
    .await
    .map_err(|error| ApiError::internal("寫入代理心跳失敗", error))
}

/// 套用一批觀測回報（見票 02、票 03、spec §HTTP API）：
/// 單一交易內 upsert 代理（算一次回報）、依 `observed_at` 舊到新套用報告
/// （sweep／passive 混批同序；未來值夾到 `now`），成功後更新
/// `last_observation_at`。
///
/// - `checked` 全部 upsert `last_checked_at`（latest-wins：離線補送的較舊
///   報告不覆寫較新的檢查時間）。
/// - `seen` 以來源 `arp` 寫 [`record_seen`]（latest-wins；MAC 已由 API 層
///   正規化為小寫冒號格式）。
/// - `senders` 只寫落在代理網段 CIDR **外**者，以來源 `arp_passive` 寫
///   [`record_passive_seen`]（`out_of_subnet = 1`；同 ADR-0017）；CIDR 內
///   丟棄，由代理主動掃描負責。
pub async fn record_observations(
    pool: &SqlitePool,
    report: &ObservationReport,
    source_ip: &str,
    subnet_id: i64,
    now: DateTime<Utc>,
) -> Result<(), ApiError> {
    let at = timestamp(now);
    let mut transaction = pool
        .begin()
        .await
        .map_err(|error| ApiError::internal("建立觀測回報交易失敗", error))?;

    upsert_agent(
        &mut transaction,
        &report.agent,
        source_ip,
        Some(subnet_id),
        &at,
    )
    .await
    .map_err(|error| ApiError::internal("寫入代理回報失敗", error))?;

    // 離線補送可能亂序：舊到新套用，確保 latest-wins 的「最後」正確。
    let mut reports: Vec<&ReportEntry> = report.reports.iter().collect();
    reports.sort_by_key(|entry| entry.observed_at());

    for entry in reports {
        // 未來時間夾到現在（見票 02 時間規則）；過去照收。
        let observed_text = timestamp(entry.observed_at().min(now));

        match entry {
            ReportEntry::Sweep(sweep) => {
                for address in &sweep.checked {
                    upsert_checked(
                        &mut transaction,
                        subnet_id,
                        &address.to_string(),
                        &observed_text,
                    )
                    .await
                    .map_err(|error| ApiError::internal("更新現況檢查時間失敗", error))?;
                }

                for (address, mac) in &sweep.seen {
                    record_seen(
                        &mut transaction,
                        subnet_id,
                        &address.to_string(),
                        Some(mac),
                        "arp",
                        &observed_text,
                    )
                    .await
                    .map_err(|error| ApiError::internal("寫入觀測現況失敗", error))?;
                }
            }
            ReportEntry::Passive(passive) => {
                for (address, mac) in &passive.senders {
                    // 只記代理網段 CIDR 外者（見票 03、ADR-0017）：CIDR 內
                    // 由代理主動掃描負責；代理 CIDR 與受管網段精確對應，故等值。
                    if report.agent.subnet_cidr.contains(&IpAddr::V4(*address)) {
                        continue;
                    }

                    record_passive_seen(
                        &mut transaction,
                        subnet_id,
                        &address.to_string(),
                        mac,
                        &observed_text,
                    )
                    .await
                    .map_err(|error| ApiError::internal("寫入網段外觀測現況失敗", error))?;
                }
            }
        }
    }

    sqlx::query("UPDATE agent SET last_observation_at = ? WHERE id = ?")
        .bind(&at)
        .bind(&report.agent.instance_id)
        .execute(&mut *transaction)
        .await
        .map_err(|error| ApiError::internal("更新代理最後觀測時間失敗", error))?;

    transaction
        .commit()
        .await
        .map_err(|error| ApiError::internal("提交觀測回報交易失敗", error))
}

/// 該網段是否有在線代理（`last_report_at` 在 `stale_secs` 內）。
///
/// 「未觀測」判定的唯一依據（見票 08、spec §讀取端）：沒有在線代理涵蓋
/// 即未觀測；v6 由呼叫端先排除。
pub async fn subnet_has_online_agent(
    pool: &SqlitePool,
    subnet_id: i64,
    now: DateTime<Utc>,
    stale_secs: u64,
) -> Result<bool, ApiError> {
    // 與 [`is_online`] 相同門檻語意：極端設定（無法表示）一律視為在線。
    let cutoff = i64::try_from(stale_secs)
        .ok()
        .and_then(|secs| now.checked_sub_signed(Duration::seconds(secs)));

    let count = match cutoff {
        Some(cutoff) => {
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM agent WHERE subnet_id = ? AND last_report_at >= ?",
            )
            .bind(subnet_id)
            .bind(timestamp(cutoff))
            .fetch_one(pool)
            .await
        }
        None => {
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM agent WHERE subnet_id = ?")
                .bind(subnet_id)
                .fetch_one(pool)
                .await
        }
    }
    .map_err(|error| ApiError::internal("讀取代理涵蓋狀態失敗", error))?;

    Ok(count > 0)
}

/// 累計一次認證失敗（來源 IP 一列）：首次與次數累加，自報名稱／版本取有值者。
///
/// `claimed_*` 為 `None`（body 解析失敗或未帶）時保留既有值，避免較差的請求
/// 抹掉先前可辨識的線索。
pub async fn record_auth_failure(
    pool: &SqlitePool,
    source_ip: &str,
    claimed_name: Option<&str>,
    claimed_version: Option<&str>,
    now: DateTime<Utc>,
) -> Result<(), ApiError> {
    let at = timestamp(now);
    sqlx::query(
        "INSERT INTO agent_auth_failure
             (source_ip, claimed_name, claimed_version, first_attempt_at, last_attempt_at,
              attempt_count)
         VALUES (?, ?, ?, ?, ?, 1)
         ON CONFLICT (source_ip) DO UPDATE SET
             claimed_name = COALESCE(excluded.claimed_name, agent_auth_failure.claimed_name),
             claimed_version = COALESCE(excluded.claimed_version, agent_auth_failure.claimed_version),
             last_attempt_at = excluded.last_attempt_at,
             attempt_count = agent_auth_failure.attempt_count + 1",
    )
    .bind(source_ip)
    .bind(claimed_name)
    .bind(claimed_version)
    .bind(&at)
    .bind(&at)
    .execute(pool)
    .await
    .map_err(|error| ApiError::internal("寫入代理被拒回報失敗", error))?;

    Ok(())
}

/// 列出代理（`last_report_at` 新到舊）並依附帶門檻判定在線。
pub async fn list(
    pool: &SqlitePool,
    now: DateTime<Utc>,
    stale_secs: u64,
) -> Result<Vec<Agent>, ApiError> {
    let rows: Vec<AgentRow> = sqlx::query_as(
        "SELECT a.id AS instance_id, a.name, a.version, a.source_ip, a.subnet_cidr,
                a.subnet_id, s.name AS subnet_name, a.first_report_at,
                a.last_report_at, a.last_observation_at
           FROM agent a
           LEFT JOIN subnets s ON s.id = a.subnet_id
          ORDER BY a.last_report_at DESC, a.id",
    )
    .fetch_all(pool)
    .await
    .map_err(|error| ApiError::internal("讀取代理清單失敗", error))?;

    Ok(rows
        .into_iter()
        .map(|row| Agent {
            online: is_online(&row.last_report_at, now, stale_secs),
            instance_id: row.instance_id,
            name: row.name,
            version: row.version,
            source_ip: row.source_ip,
            subnet_cidr: row.subnet_cidr,
            subnet_id: row.subnet_id,
            subnet_name: row.subnet_name,
            first_report_at: row.first_report_at,
            last_report_at: row.last_report_at,
            last_observation_at: row.last_observation_at,
        })
        .collect())
}

/// 列出被拒回報（`last_attempt_at` 新到舊）。
pub async fn list_auth_failures(pool: &SqlitePool) -> Result<Vec<AuthFailure>, ApiError> {
    let rows: Vec<AuthFailureRow> = sqlx::query_as(
        "SELECT source_ip, claimed_name, claimed_version, first_attempt_at, last_attempt_at,
                attempt_count
           FROM agent_auth_failure
          ORDER BY last_attempt_at DESC, source_ip",
    )
    .fetch_all(pool)
    .await
    .map_err(|error| ApiError::internal("讀取代理被拒回報失敗", error))?;

    Ok(rows
        .into_iter()
        .map(|row| AuthFailure {
            source_ip: row.source_ip,
            claimed_name: row.claimed_name,
            claimed_version: row.claimed_version,
            first_attempt_at: row.first_attempt_at,
            last_attempt_at: row.last_attempt_at,
            attempt_count: row.attempt_count,
        })
        .collect())
}

/// 保留清理：刪除早於保留期的被拒回報列，回傳刪除筆數（見票 08）。
///
/// 界線同事件清理（[`crate::observation::cleanup_events`]）：
/// `cutoff = now - retention_days`；刪除條件為 `last_attempt_at < cutoff`
/// （嚴格早於），恰在 cutoff 的列保留。以最後嘗試時間為準：仍持續嘗試的
/// 來源即使首次嘗試很久以前也不會被清掉。
pub async fn cleanup_auth_failures(
    pool: &SqlitePool,
    retention_days: u32,
    now: DateTime<Utc>,
) -> Result<u64, ApiError> {
    let cutoff = now
        .checked_sub_signed(Duration::days(i64::from(retention_days)))
        .ok_or_else(|| ApiError::internal("換算被拒回報保留期限失敗", retention_days))?;

    let result = sqlx::query("DELETE FROM agent_auth_failure WHERE last_attempt_at < ?")
        .bind(timestamp(cutoff))
        .execute(pool)
        .await
        .map_err(|error| ApiError::internal("清理代理被拒回報失敗", error))?;

    Ok(result.rows_affected())
}

/// 在線判定：`last_report_at` 不早於 `now - stale_secs`（含邊界；見票 01）。
///
/// 資料庫時間為固定格式 `YYYY-MM-DDTHH:MM:SSZ`（UTC），字串比較即時間比較；
/// `stale_secs` 超出可表示範圍時（極端設定）一律視為在線。
pub fn is_online(last_report_at: &str, now: DateTime<Utc>, stale_secs: u64) -> bool {
    match i64::try_from(stale_secs)
        .ok()
        .and_then(|secs| now.checked_sub_signed(Duration::seconds(secs)))
    {
        Some(cutoff) => last_report_at >= timestamp(cutoff).as_str(),
        None => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn online_uses_threshold_with_inclusive_boundary() {
        let now = Utc.with_ymd_and_hms(2026, 10, 6, 12, 0, 0).unwrap();

        assert!(
            is_online("2026-10-06T11:45:00Z", now, 900),
            "恰在門檻邊界視為在線（不超過）"
        );
        assert!(
            is_online("2026-10-06T11:45:01Z", now, 900),
            "門檻內視為在線"
        );
        assert!(
            !is_online("2026-10-06T11:44:59Z", now, 900),
            "早於門檻即離線"
        );
        assert!(
            is_online("2026-10-06T12:30:00Z", now, 900),
            "未來時間（時鐘偏差）視為在線"
        );
    }

    #[test]
    fn online_treats_unrepresentable_threshold_as_online() {
        let now = Utc.with_ymd_and_hms(2026, 10, 6, 12, 0, 0).unwrap();

        assert!(
            is_online("1970-01-01T00:00:00Z", now, u64::MAX),
            "門檻大到無法表示時不誤判離線"
        );
    }
}
