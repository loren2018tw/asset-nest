//! IP 觀測：現況讀取與快速掃描服務（見票 02、spec §掃描服務、ADR-0015／0016）。
//!
//! 觀測是唯讀影子層：掃描只寫 `ip_presence`（現況）與 `observation_event`
//! （變化事件），永不修改宣告資料（見 ADR-0014）。排程器（票 05 之後）只是
//! 呼叫 [`run_quick`]／`run_discovery` 的薄迴圈；本模組服務可直接呼叫。

use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::sync::Arc;
use std::time::Instant;

use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{FromRow, SqliteConnection, SqlitePool};

use crate::api::ApiError;
use crate::assignments;
use crate::probe::{Mac, Prober};
use crate::subnets::Subnet;

/// `ip_presence` 現況列（讀取端；見 ADR-0016）。
#[derive(Debug, Clone, Default)]
pub struct Presence {
    pub last_seen_at: Option<String>,
    pub last_seen_mac: Option<String>,
    pub last_seen_source: Option<String>,
    pub last_checked_at: Option<String>,
}

/// `presence_map` 的查詢列：現況＋所屬位址（map 鍵）。
#[derive(Debug, FromRow)]
struct PresenceRow {
    address: String,
    last_seen_at: Option<String>,
    last_seen_mac: Option<String>,
    last_seen_source: Option<String>,
    last_checked_at: Option<String>,
}

/// IP 清單的觀測視圖：有效涵蓋＋各列現況（以位址文字索引）。
///
/// 有效涵蓋＝網段 `observed` ∧ 本機同 L2 ∧ v4（由呼叫端計算；v6 恆 false）。
#[derive(Debug, Default)]
pub struct ObservationView {
    pub observed: bool,
    pub presence: HashMap<String, Presence>,
}

/// 掃描摘要（HTTP 回應；見 spec §HTTP API）。
#[derive(Debug, Serialize)]
pub struct SweepReport {
    pub mode: &'static str,
    /// 本次探測的目標位址數。
    pub targets: u64,
    /// 有任一來源回應的目標位址數（ARP 回應；票 03 加入 Kea 租約）。
    pub seen: u64,
    /// 掃描耗時（毫秒；含探測回覆窗與寫入）。
    pub duration_ms: u64,
}

/// 資料庫時間格式：`YYYY-MM-DDTHH:MM:SSZ`（UTC；見 spec §資料庫）。
pub fn timestamp(now: DateTime<Utc>) -> String {
    now.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// 批次讀取某網段的現況列（單一查詢；供 IP 清單左併、避免 N+1）。
pub async fn presence_map(
    pool: &SqlitePool,
    subnet_id: i64,
) -> Result<HashMap<String, Presence>, ApiError> {
    let rows: Vec<PresenceRow> = sqlx::query_as(
        "SELECT address, last_seen_at, last_seen_mac, last_seen_source, last_checked_at
           FROM ip_presence
          WHERE subnet_id = ?",
    )
    .bind(subnet_id)
    .fetch_all(pool)
    .await
    .map_err(|error| ApiError::internal("讀取觀測現況失敗", error))?;

    Ok(rows
        .into_iter()
        .map(|row| {
            (
                row.address,
                Presence {
                    last_seen_at: row.last_seen_at,
                    last_seen_mac: row.last_seen_mac,
                    last_seen_source: row.last_seen_source,
                    last_checked_at: row.last_checked_at,
                },
            )
        })
        .collect())
}

/// 快速掃描：探測該網段的已指派位址，更新現況並依變化寫事件（見票 02）。
///
/// - 前提：`observed`、v4、`prober.is_local`；否則回 400 驗證錯誤。
/// - 目標：該網段已指派位址（票 03 擴充 Kea 目前有效租約）。
/// - 所有目標 upsert `last_checked_at = now`；回應者寫 `last_seen_*`
///   （來源 `arp`，latest-wins：較舊的觀測不覆寫較新的現況）。
/// - 事件只在變化時寫：首次看到 `first_seen`、換 MAC `mac_changed`。
/// - 探測為阻塞式，以 `tokio::task::spawn_blocking` 執行。
pub async fn run_quick(
    pool: &SqlitePool,
    prober: Arc<dyn Prober + Send + Sync>,
    subnet: &Subnet,
    now: DateTime<Utc>,
) -> Result<SweepReport, ApiError> {
    validate_quick(subnet, prober.as_ref())?;

    let started = Instant::now();
    let targets = assigned_targets(pool, subnet).await?;

    let responses = if targets.is_empty() {
        Vec::new()
    } else {
        let probe_prober = Arc::clone(&prober);
        let probe_subnet = subnet.clone();
        let probe_targets = targets.clone();
        tokio::task::spawn_blocking(move || probe_prober.probe(&probe_subnet, &probe_targets))
            .await
            .map_err(|error| ApiError::internal("快速掃描工作失敗", error))?
    };

    apply_results(pool, subnet.id, &targets, &responses, now).await?;

    let seen = responses
        .iter()
        .filter(|(address, _)| targets.contains(address))
        .count() as u64;

    Ok(SweepReport {
        mode: "quick",
        targets: targets.len() as u64,
        seen,
        duration_ms: started.elapsed().as_millis() as u64,
    })
}

/// 快速掃描前提：v4、已開觀測、本機同 L2。
fn validate_quick(subnet: &Subnet, prober: &dyn Prober) -> Result<(), ApiError> {
    if subnet.cidr.contains(':') {
        return Err(ApiError::validation("IPv6 網段不支援觀測").field("observed"));
    }
    if !subnet.observed {
        return Err(ApiError::validation("此網段未開啟觀測").field("observed"));
    }
    if !prober.is_local(subnet) {
        return Err(ApiError::validation("本機與此網段非同 L2，無法觀測").field("local"));
    }
    Ok(())
}

/// 快速掃描目標：該網段已指派位址（去重、數值升冪）。
///
/// 票 03 將在此聯集 Kea 目前有效（`state=default`）租約位址。
async fn assigned_targets(pool: &SqlitePool, subnet: &Subnet) -> Result<Vec<Ipv4Addr>, ApiError> {
    let assignments = assignments::list_for_subnet(pool, subnet.id)
        .await
        .map_err(|error| ApiError::internal("讀取指派清單失敗", error))?;

    let mut targets: Vec<Ipv4Addr> = Vec::with_capacity(assignments.len());
    for assignment in &assignments {
        let address: Ipv4Addr = assignment
            .address
            .parse()
            .map_err(|error| ApiError::internal("指派位址格式錯誤", error))?;
        if !targets.contains(&address) {
            targets.push(address);
        }
    }
    targets.sort_unstable();
    Ok(targets)
}

/// 套用一次掃描的結果：所有目標更新 `last_checked_at`，回應者依「變化才寫」
/// 更新現況與事件（同一交易）。
async fn apply_results(
    pool: &SqlitePool,
    subnet_id: i64,
    targets: &[Ipv4Addr],
    responses: &[(Ipv4Addr, Mac)],
    now: DateTime<Utc>,
) -> Result<(), ApiError> {
    let now_text = timestamp(now);
    let mut transaction = pool
        .begin()
        .await
        .map_err(|error| ApiError::internal("建立掃描交易失敗", error))?;

    for target in targets {
        upsert_checked(&mut transaction, subnet_id, &target.to_string(), &now_text)
            .await
            .map_err(|error| ApiError::internal("更新現況檢查時間失敗", error))?;
    }

    for (address, mac) in responses {
        let Some(normalized) = crate::probe::normalize_mac(mac) else {
            tracing::warn!(mac = %mac, "忽略格式異常的探測 MAC");
            continue;
        };
        record_seen(
            &mut transaction,
            subnet_id,
            &address.to_string(),
            &normalized,
            "arp",
            &now_text,
        )
        .await
        .map_err(|error| ApiError::internal("寫入觀測現況失敗", error))?;
    }

    transaction
        .commit()
        .await
        .map_err(|error| ApiError::internal("提交掃描交易失敗", error))
}

/// upsert 目標的 `last_checked_at`；既有 `last_seen_*` 不受影響。
async fn upsert_checked(
    connection: &mut SqliteConnection,
    subnet_id: i64,
    address: &str,
    checked_at: &str,
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO ip_presence (subnet_id, address, last_checked_at)
         VALUES (?, ?, ?)
         ON CONFLICT (subnet_id, address)
             DO UPDATE SET last_checked_at = excluded.last_checked_at",
    )
    .bind(subnet_id)
    .bind(address)
    .bind(checked_at)
    .execute(&mut *connection)
    .await?;
    Ok(())
}

/// 現況轉移 → 事件種類：首次看到 `first_seen`、換 MAC `mac_changed`、
/// 同 MAC（不分大小寫）不寫事件（見 spec §掃描服務）。
pub(crate) fn transition_event(previous_mac: Option<&str>, new_mac: &str) -> Option<&'static str> {
    match previous_mac {
        None => Some("first_seen"),
        Some(previous) if !previous.eq_ignore_ascii_case(new_mac) => Some("mac_changed"),
        Some(_) => None,
    }
}

/// 寫入單筆「看到」：依現況轉移寫事件（append-only），latest-wins 更新現況。
///
/// `observed_at` 較舊（如票 03 的 Kea `cltt`）時不覆寫較新的現況，但同值
/// 時間（>=）仍更新來源；事件依 MAC 變化決定、與時間新舊無關（歷史事實）。
///
/// 票 03（Kea 租約來源）以同一介面併入：來源帶 `kea_lease`、時間帶 `cltt`，
/// 租約位址亦須先納入目標集合（呼叫 [`upsert_checked`] 更新 `last_checked_at`）。
pub(crate) async fn record_seen(
    connection: &mut SqliteConnection,
    subnet_id: i64,
    address: &str,
    mac: &str,
    source: &str,
    observed_at: &str,
) -> sqlx::Result<()> {
    let current: Option<(Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT last_seen_at, last_seen_mac FROM ip_presence WHERE subnet_id = ? AND address = ?",
    )
    .bind(subnet_id)
    .bind(address)
    .fetch_optional(&mut *connection)
    .await?;

    let previous_mac = current
        .as_ref()
        .and_then(|(_, stored_mac)| stored_mac.as_deref());

    if let Some(kind) = transition_event(previous_mac, mac) {
        sqlx::query(
            "INSERT INTO observation_event (subnet_id, address, mac, kind, source, observed_at)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(subnet_id)
        .bind(address)
        .bind(mac)
        .bind(kind)
        .bind(source)
        .bind(observed_at)
        .execute(&mut *connection)
        .await?;
    }

    match current {
        Some((previous_at, _)) => {
            // latest-wins：僅在觀測不舊於現況時覆寫（票 03 租約 cltt 同路徑）。
            let newer = previous_at
                .as_deref()
                .is_none_or(|previous| observed_at >= previous);
            if newer {
                sqlx::query(
                    "UPDATE ip_presence
                        SET last_seen_at = ?, last_seen_mac = ?, last_seen_source = ?
                      WHERE subnet_id = ? AND address = ?",
                )
                .bind(observed_at)
                .bind(mac)
                .bind(source)
                .bind(subnet_id)
                .bind(address)
                .execute(&mut *connection)
                .await?;
            }
        }
        None => {
            sqlx::query(
                "INSERT INTO ip_presence
                     (subnet_id, address, last_seen_at, last_seen_mac, last_seen_source)
                 VALUES (?, ?, ?, ?, ?)",
            )
            .bind(subnet_id)
            .bind(address)
            .bind(observed_at)
            .bind(mac)
            .bind(source)
            .execute(&mut *connection)
            .await?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transition_event_covers_first_seen_change_and_same_mac() {
        assert_eq!(
            transition_event(None, "aa:bb:cc:dd:ee:ff"),
            Some("first_seen"),
            "首次有 last_seen_mac 寫 first_seen"
        );
        assert_eq!(
            transition_event(Some("aa:bb:cc:dd:ee:ff"), "aa:bb:cc:dd:ee:ff"),
            None,
            "同 MAC 不寫事件"
        );
        assert_eq!(
            transition_event(Some("AA:BB:CC:DD:EE:FF"), "aa:bb:cc:dd:ee:ff"),
            None,
            "MAC 比較不分大小寫"
        );
        assert_eq!(
            transition_event(Some("aa:bb:cc:dd:ee:ff"), "aa:bb:cc:dd:ee:00"),
            Some("mac_changed"),
            "換 MAC 寫 mac_changed"
        );
    }

    #[test]
    fn timestamp_uses_spec_utc_format() {
        let now = DateTime::parse_from_rfc3339("2026-10-06T12:34:56Z")
            .expect("固定時間")
            .with_timezone(&Utc);
        assert_eq!(timestamp(now), "2026-10-06T12:34:56Z");
    }
}
