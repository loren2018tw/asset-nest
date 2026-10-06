//! IP 觀測：觀測入庫、現況／事件推導與保留清理（見 ADR-0014、ADR-0016）。
//!
//! 觀測是唯讀影子層：寫入只動 `ip_presence`（現況）與 `observation_event`
//! （變化事件），永不修改宣告資料（見 ADR-0014）。探測執行已外移給觀測代理
//! （見 ADR-0018）；後端只保留 Kea 租約觀測（[`record_lease_observations`]）、
//! 代理入庫共用的寫入原語與事件保留清理。

use std::collections::{HashMap, HashSet};
use std::net::Ipv4Addr;

use chrono::{DateTime, Utc};
use ipnet::Ipv4Net;
use serde::Serialize;
use sqlx::{FromRow, SqliteConnection, SqlitePool};

use crate::agents;
use crate::api::ApiError;
use crate::kea::http::{Client as KeaClient, KeaLease};
use crate::subnets::Subnet;

/// `ip_presence` 現況列（讀取端；見 ADR-0016）。
#[derive(Debug, Clone, Default, Serialize)]
pub struct Presence {
    pub last_seen_at: Option<String>,
    pub last_seen_mac: Option<String>,
    pub last_seen_source: Option<String>,
    pub last_checked_at: Option<String>,
}

/// `observation_event` 一列（歷史端點與 CSV 匯出共用；見票 06）。
///
/// `address` 僅供 CSV 匯出使用：IP 歷史端點的路徑已含位址，JSON 回應不重複。
#[derive(Debug, FromRow, Serialize)]
pub struct ObservationEvent {
    pub id: i64,
    #[serde(skip_serializing)]
    pub address: String,
    pub mac: Option<String>,
    pub kind: String,
    pub source: String,
    pub observed_at: String,
}

/// 觀測 MAC 連結到的資產摘要（已知 MAC 的顯示資訊；見票 06）。
#[derive(Debug, Clone, Serialize)]
pub struct MacAsset {
    pub id: i64,
    pub description: String,
    pub location: String,
    pub property_no: Option<String>,
}

/// 某位址用過的 MAC 彙總列（IP 歷史回應；見票 06）。
#[derive(Debug, Serialize)]
pub struct UsedMac {
    pub mac: String,
    pub first_seen_at: String,
    pub last_seen_at: String,
    pub source: Option<String>,
    pub known: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asset: Option<MacAsset>,
}

/// IP 歷史回應：有效涵蓋＋現況＋事件（新到舊）＋用過的 MAC（見票 06）。
#[derive(Debug, Serialize)]
pub struct IpHistory {
    pub observed: bool,
    pub presence: Option<Presence>,
    pub events: Vec<ObservationEvent>,
    pub macs: Vec<UsedMac>,
}

/// MAC 歷史中的單一位址 sightings 列：該位址首見、最後可見、來源（見 spec §讀取端）。
#[derive(Debug, Serialize)]
pub struct MacSighting {
    pub address: String,
    pub first_seen_at: String,
    pub last_seen_at: String,
    pub source: Option<String>,
}

/// MAC 歷史回應：用過哪些位址＋是否已知（含連結資產；見票 06）。
#[derive(Debug, Serialize)]
pub struct MacHistory {
    pub mac: String,
    pub known: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asset: Option<MacAsset>,
    pub sightings: Vec<MacSighting>,
}

/// 網段外觀測清單的一列（`GET /api/v1/observations/out-of-subnet`；見票 01）。
#[derive(Debug, Serialize)]
pub struct OutOfSubnetObservation {
    pub subnet_id: i64,
    pub subnet_cidr: String,
    pub subnet_name: Option<String>,
    pub address: String,
    pub mac: Option<String>,
    /// 該列最早事件時間；事件經保留清理後退化為 `last_seen_at`。
    pub first_seen_at: String,
    pub last_seen_at: String,
    pub source: Option<String>,
    pub known: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asset: Option<MacAsset>,
}

/// 網段外觀測清單回應。
#[derive(Debug, Serialize)]
pub struct OutOfSubnetList {
    pub items: Vec<OutOfSubnetObservation>,
}

/// [`assets_for_macs`] 的查詢列。
#[derive(Debug, FromRow)]
struct LinkedMacRow {
    mac: String,
    id: i64,
    description: String,
    location: String,
    property_no: Option<String>,
}

/// [`out_of_subnet_observations`] 的查詢列。
#[derive(Debug, FromRow)]
struct OutOfSubnetRow {
    subnet_id: i64,
    subnet_cidr: String,
    subnet_name: Option<String>,
    address: String,
    mac: Option<String>,
    first_seen_at: String,
    last_seen_at: String,
    source: Option<String>,
}

/// [`used_macs`] 的中間訊號：同一 MAC 的首見／最後可見／來源。
#[derive(Debug)]
struct MacSignal {
    mac: String,
    first_seen_at: String,
    last_seen_at: String,
    source: Option<String>,
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

/// IP 清單的觀測視圖：有效涵蓋＋各列現況（以位址文字索引）＋已登錄 MAC 集合。
///
/// 有效涵蓋＝v4 且該網段有在線代理（見 [`effective_coverage`]）；v6 恆 false。
/// `known_macs` 為全系統 Interface MAC（小寫；見 [`crate::interfaces::macs`]），
/// 供 `unknown_mac` 篩選；不以該篩選查詢時可留空集合。
#[derive(Debug, Default)]
pub struct ObservationView {
    pub observed: bool,
    pub presence: HashMap<String, Presence>,
    pub known_macs: HashSet<String>,
}

/// 資料庫時間格式：`YYYY-MM-DDTHH:MM:SSZ`（UTC；見 spec §資料庫）。
pub fn timestamp(now: DateTime<Utc>) -> String {
    now.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// 正規化 MAC 文字為小寫冒號格式；格式不符回 `None`。
///
/// 接受 `-` 或 `:` 分隔的 6／8 組兩位十六進位（大小寫不拘；Ethernet 與
/// EUI-64，比照 Kea `hw-address` 與代理回報格式）。
pub(crate) fn normalize_mac(text: &str) -> Option<String> {
    let normalized = text.trim().to_ascii_lowercase().replace('-', ":");
    let groups: Vec<&str> = normalized.split(':').collect();
    if !matches!(groups.len(), 6 | 8)
        || !groups
            .iter()
            .all(|group| group.len() == 2 && group.chars().all(|c| c.is_ascii_hexdigit()))
    {
        return None;
    }
    Some(groups.join(":"))
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

/// 有效觀測涵蓋（見票 08、spec §讀取端）：v4 且該網段有在線代理。
///
/// 代理在線＝`last_report_at` 在 `stale_secs` 內（見
/// [`crate::agents::subnet_has_online_agent`]）；v6 恆未觀測。
pub async fn effective_coverage(
    pool: &SqlitePool,
    subnet: &Subnet,
    now: DateTime<Utc>,
    stale_secs: u64,
) -> Result<bool, ApiError> {
    if subnet.cidr.contains(':') {
        return Ok(false);
    }
    agents::subnet_has_online_agent(pool, subnet.id, now, stale_secs).await
}

/// 讀取單一位址的觀測歷史（見票 06、spec §讀取端）：現況、事件（新到舊）與
/// 用過的 MAC 彙總；`observed` 為該網段的有效涵蓋（比照 IP 清單）。
///
/// 彙總規則（MAC 非實體、由事件與現況推導；見 ADR-0016）：
/// - 相異 MAC＝該位址事件的 MAC ∪ 現況 `last_seen_mac`（事件經保留清理後，
///   現況仍保留目前 MAC 的痕跡）。
/// - 首見＝該 MAC 最早的事件時間；無事件時以現況時間為準。
/// - 最後可見＝該 MAC 最晚的事件時間；若該 MAC 即現況 `last_seen_mac` 且
///   現況時間不舊於事件，取現況時間（現況是目前狀態、平手時勝出）。
/// - 來源＝最後可見訊號的來源。
/// - 已知＝MAC 存在於任一 Interface（不分大小寫）；連結資產取第一筆命中。
pub async fn ip_history(
    pool: &SqlitePool,
    subnet: &Subnet,
    address: &str,
    now: DateTime<Utc>,
    stale_secs: u64,
) -> Result<IpHistory, ApiError> {
    let events = list_events_for_address(pool, subnet.id, address).await?;
    let presence = presence_for_address(pool, subnet.id, address).await?;
    let macs = used_macs(pool, &events, presence.as_ref()).await?;
    let observed = effective_coverage(pool, subnet, now, stale_secs).await?;

    Ok(IpHistory {
        observed,
        presence,
        events,
        macs,
    })
}

/// 讀取某 MAC 的觀測歷史（見票 06、spec §讀取端）：用過的位址與各自
/// 首見／最後可見／來源，另回傳是否已知與連結資產。
///
/// 彙總規則（跨網段；位址不重複出現於多網段，故以位址彙總）：
/// - 相異位址＝該 MAC 的事件位址 ∪ 現況 `last_seen_mac` 命中的位址
///   （事件經保留清理後，現況仍保留目前痕跡）。
/// - 首見＝該位址最早的該 MAC 事件時間；無事件時以現況時間為準。
/// - 最後可見＝該位址最晚的事件時間；若現況 MAC 相同且現況時間不舊於
///   事件，取現況時間（現況平手時勝出）。
/// - 來源＝最後可見訊號的來源。
///
/// MAC 比較不分大小寫；`mac` 由呼叫端以 [`normalize_mac`] 正規化後傳入
/// （無效值於 API 層回 400）。
pub async fn mac_history(pool: &SqlitePool, mac: &str) -> Result<MacHistory, ApiError> {
    let normalized = mac.to_ascii_lowercase();

    // 事件舊到新；同秒事件依 id（後寫者為較新）。
    let events: Vec<ObservationEvent> = sqlx::query_as(
        "SELECT id, address, mac, kind, source, observed_at
           FROM observation_event
          WHERE LOWER(mac) = ?
          ORDER BY observed_at ASC, id ASC",
    )
    .bind(&normalized)
    .fetch_all(pool)
    .await
    .map_err(|error| ApiError::internal("讀取 MAC 觀測事件失敗", error))?;

    let presences: Vec<PresenceAddressRow> = sqlx::query_as(
        "SELECT address, last_seen_at, last_seen_source
           FROM ip_presence
          WHERE LOWER(last_seen_mac) = ?",
    )
    .bind(&normalized)
    .fetch_all(pool)
    .await
    .map_err(|error| ApiError::internal("讀取 MAC 觀測現況失敗", error))?;

    let mut sightings: Vec<MacSighting> = Vec::new();
    for event in &events {
        match sightings
            .iter_mut()
            .find(|sighting| sighting.address == event.address)
        {
            Some(sighting) => {
                sighting.last_seen_at = event.observed_at.clone();
                sighting.source = Some(event.source.clone());
            }
            None => sightings.push(MacSighting {
                address: event.address.clone(),
                first_seen_at: event.observed_at.clone(),
                last_seen_at: event.observed_at.clone(),
                source: Some(event.source.clone()),
            }),
        }
    }

    for presence in &presences {
        // 現況只有 last_seen_at 非空時才是「看到」的訊號。
        let Some(seen_at) = presence.last_seen_at.as_deref() else {
            continue;
        };
        match sightings
            .iter_mut()
            .find(|sighting| sighting.address == presence.address)
        {
            Some(sighting) => {
                if seen_at >= sighting.last_seen_at.as_str() {
                    sighting.last_seen_at = seen_at.to_string();
                    sighting.source = presence.last_seen_source.clone();
                }
            }
            None => sightings.push(MacSighting {
                address: presence.address.clone(),
                first_seen_at: seen_at.to_string(),
                last_seen_at: seen_at.to_string(),
                source: presence.last_seen_source.clone(),
            }),
        }
    }

    // 顯示順序：最後可見新到舊；同時間依位址文字。
    sightings.sort_by(|a, b| {
        b.last_seen_at
            .cmp(&a.last_seen_at)
            .then_with(|| a.address.cmp(&b.address))
    });

    let asset = assets_for_macs(pool, std::slice::from_ref(&normalized))
        .await?
        .remove(&normalized);

    Ok(MacHistory {
        mac: normalized,
        known: asset.is_some(),
        asset,
        sightings,
    })
}

/// 讀取網段外觀測清單（見票 01、ADR-0017）：`out_of_subnet = 1` 的現況列，
/// last_seen 新到舊（同時間依網段、位址）。
///
/// - `first_seen_at`＝該 `(subnet_id, address)` 最早事件時間；事件經保留清理
///   後退化為 `last_seen_at`（現況不受清理影響）。
/// - `known`／`asset` 比照 MAC 歷史：MAC 存在於任一 Interface 即已知，連結
///   取第一筆命中（不分大小寫）。
/// - 殘留列自癒：網段 CIDR 之後擴大而涵蓋該位址時，於組裝回應時跳過該列
///   （資料庫旗標不動；仍在外者照常列出）。
/// - 唯讀；資料只由觀測代理的被動監聽產生。
pub async fn out_of_subnet_observations(pool: &SqlitePool) -> Result<OutOfSubnetList, ApiError> {
    let rows: Vec<OutOfSubnetRow> = sqlx::query_as(
        "SELECT p.subnet_id,
                s.cidr AS subnet_cidr,
                s.name AS subnet_name,
                p.address,
                p.last_seen_mac AS mac,
                p.last_seen_at AS last_seen_at,
                p.last_seen_source AS source,
                COALESCE(
                    (SELECT MIN(e.observed_at)
                       FROM observation_event e
                      WHERE e.subnet_id = p.subnet_id AND e.address = p.address),
                    p.last_seen_at
                ) AS first_seen_at
           FROM ip_presence p
           JOIN subnets s ON s.id = p.subnet_id
          WHERE p.out_of_subnet = 1
          ORDER BY p.last_seen_at DESC, p.subnet_id ASC, p.address ASC",
    )
    .fetch_all(pool)
    .await
    .map_err(|error| ApiError::internal("讀取網段外觀測清單失敗", error))?;

    let macs: Vec<String> = rows.iter().filter_map(|row| row.mac.clone()).collect();
    let assets = assets_for_macs(pool, &macs).await?;

    let items = rows
        .into_iter()
        .filter(|row| still_outside_subnet(&row.subnet_cidr, &row.address))
        .map(|row| {
            let asset = row
                .mac
                .as_deref()
                .and_then(|mac| assets.get(&mac.to_ascii_lowercase()).cloned());
            OutOfSubnetObservation {
                subnet_id: row.subnet_id,
                subnet_cidr: row.subnet_cidr,
                subnet_name: row.subnet_name,
                address: row.address,
                mac: row.mac,
                first_seen_at: row.first_seen_at,
                last_seen_at: row.last_seen_at,
                source: row.source,
                known: asset.is_some(),
                asset,
            }
        })
        .collect();

    Ok(OutOfSubnetList { items })
}

/// 網段外觀測列的殘留自癒判斷（純函式；見票 01）：位址已落在網段 CIDR 內
/// 時不再列出；仍在外者保留。CIDR 或位址無法解析時保守保留（不隱藏資料）。
fn still_outside_subnet(subnet_cidr: &str, address: &str) -> bool {
    let (Ok(network), Ok(address)) = (subnet_cidr.parse::<Ipv4Net>(), address.parse::<Ipv4Addr>())
    else {
        return true;
    };
    !network.contains(&address)
}

/// 產生單一 IP 觀測歷史匯出 CSV（UTF-8 BOM＋標題列；見票 06、spec §HTTP API）。
///
/// 欄位 `address, mac, kind, source, observed_at`；資料列與 IP 歷史端點一致、
/// 為事件新到舊（時間軸的閱讀順序）。`mac` 缺漏輸出空字串。
pub fn history_export_csv(events: &[ObservationEvent]) -> Result<Vec<u8>, ApiError> {
    let mut writer = csv::Writer::from_writer(Vec::new());
    writer
        .write_record(["address", "mac", "kind", "source", "observed_at"])
        .map_err(history_write_error)?;

    for event in events {
        writer
            .write_record([
                event.address.as_str(),
                event.mac.as_deref().unwrap_or(""),
                event.kind.as_str(),
                event.source.as_str(),
                event.observed_at.as_str(),
            ])
            .map_err(history_write_error)?;
    }

    let body = writer
        .into_inner()
        .map_err(|error| ApiError::internal("產生觀測歷史 CSV 失敗", error))?;
    let mut bytes = Vec::with_capacity(body.len() + 3);
    bytes.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
    bytes.extend_from_slice(&body);
    Ok(bytes)
}

/// CSV 寫入錯誤一律視為內部錯誤（寫入目標為記憶體緩衝區）。
fn history_write_error(error: csv::Error) -> ApiError {
    ApiError::internal("產生觀測歷史 CSV 失敗", error)
}

/// 讀取該位址的事件（新到舊；同秒依 id 後寫者在前）。
async fn list_events_for_address(
    pool: &SqlitePool,
    subnet_id: i64,
    address: &str,
) -> Result<Vec<ObservationEvent>, ApiError> {
    sqlx::query_as(
        "SELECT id, address, mac, kind, source, observed_at
           FROM observation_event
          WHERE subnet_id = ? AND address = ?
          ORDER BY observed_at DESC, id DESC",
    )
    .bind(subnet_id)
    .bind(address)
    .fetch_all(pool)
    .await
    .map_err(|error| ApiError::internal("讀取觀測事件失敗", error))
}

/// 讀取該位址的現況列；不存在回 `None`。
async fn presence_for_address(
    pool: &SqlitePool,
    subnet_id: i64,
    address: &str,
) -> Result<Option<Presence>, ApiError> {
    let row: Option<PresenceRow> = sqlx::query_as(
        "SELECT address, last_seen_at, last_seen_mac, last_seen_source, last_checked_at
           FROM ip_presence
          WHERE subnet_id = ? AND address = ?",
    )
    .bind(subnet_id)
    .bind(address)
    .fetch_optional(pool)
    .await
    .map_err(|error| ApiError::internal("讀取觀測現況失敗", error))?;

    Ok(row.map(|row| Presence {
        last_seen_at: row.last_seen_at,
        last_seen_mac: row.last_seen_mac,
        last_seen_source: row.last_seen_source,
        last_checked_at: row.last_checked_at,
    }))
}

/// 彙總某位址用過的 MAC（規則見 [`ip_history`]）。
async fn used_macs(
    pool: &SqlitePool,
    events: &[ObservationEvent],
    presence: Option<&Presence>,
) -> Result<Vec<UsedMac>, ApiError> {
    // 事件為新到舊；反轉為舊到新逐筆推進，最後一筆即該 MAC 的最新訊號
    // （同秒時 id 大者後寫、勝出）。
    let mut signals: Vec<MacSignal> = Vec::new();
    for event in events.iter().rev() {
        let Some(mac) = event.mac.as_deref() else {
            continue;
        };
        let mac = mac.to_ascii_lowercase();
        match signals.iter_mut().find(|signal| signal.mac == mac) {
            Some(signal) => {
                signal.last_seen_at = event.observed_at.clone();
                signal.source = Some(event.source.clone());
            }
            None => signals.push(MacSignal {
                mac,
                first_seen_at: event.observed_at.clone(),
                last_seen_at: event.observed_at.clone(),
                source: Some(event.source.clone()),
            }),
        }
    }

    // 現況是目前狀態：時間平手時勝出；事件已清理時仍能看到目前 MAC。
    if let Some(presence) = presence
        && let (Some(mac), Some(seen_at)) = (
            presence.last_seen_mac.as_deref(),
            presence.last_seen_at.as_deref(),
        )
    {
        let mac = mac.to_ascii_lowercase();
        match signals.iter_mut().find(|signal| signal.mac == mac) {
            Some(signal) => {
                if seen_at >= signal.last_seen_at.as_str() {
                    signal.last_seen_at = seen_at.to_string();
                    signal.source = presence.last_seen_source.clone();
                }
            }
            None => signals.push(MacSignal {
                mac,
                first_seen_at: seen_at.to_string(),
                last_seen_at: seen_at.to_string(),
                source: presence.last_seen_source.clone(),
            }),
        }
    }

    let macs: Vec<String> = signals.iter().map(|signal| signal.mac.clone()).collect();
    let assets = assets_for_macs(pool, &macs).await?;

    let mut used: Vec<UsedMac> = signals
        .into_iter()
        .map(|signal| {
            let asset = assets.get(&signal.mac).cloned();
            UsedMac {
                known: asset.is_some(),
                mac: signal.mac,
                first_seen_at: signal.first_seen_at,
                last_seen_at: signal.last_seen_at,
                source: signal.source,
                asset,
            }
        })
        .collect();

    // 顯示順序：最後可見新到舊；同時間依 MAC 文字。
    used.sort_by(|a, b| {
        b.last_seen_at
            .cmp(&a.last_seen_at)
            .then_with(|| a.mac.cmp(&b.mac))
    });
    Ok(used)
}

/// 批次查詢 MAC 對應的資產（不分大小寫；同一 MAC 多筆介面取 id 最小者）。
///
/// 回傳以正規化小寫 MAC 為鍵；未命中（未知 MAC）者不在 map 中。
async fn assets_for_macs(
    pool: &SqlitePool,
    macs: &[String],
) -> Result<HashMap<String, MacAsset>, ApiError> {
    if macs.is_empty() {
        return Ok(HashMap::new());
    }

    let mut builder = sqlx::QueryBuilder::new(
        "SELECT LOWER(i.mac) AS mac, i.asset_id AS id, a.description, a.location, a.property_no
           FROM interfaces i
           JOIN assets a ON a.id = i.asset_id
          WHERE LOWER(i.mac) IN (",
    );
    let mut separated = builder.separated(", ");
    for mac in macs {
        separated.push_bind(mac.to_ascii_lowercase());
    }
    builder.push(") ORDER BY i.id ASC");

    let rows: Vec<LinkedMacRow> = builder
        .build_query_as()
        .fetch_all(pool)
        .await
        .map_err(|error| ApiError::internal("讀取 MAC 對應資產失敗", error))?;

    let mut assets = HashMap::new();
    for row in rows {
        // `ORDER BY i.id ASC`：同 MAC 多筆介面時第一筆勝出。
        assets.entry(row.mac).or_insert(MacAsset {
            id: row.id,
            description: row.description,
            location: row.location,
            property_no: row.property_no,
        });
    }
    Ok(assets)
}

/// [`mac_history`] 的現況查詢列。
#[derive(Debug, FromRow)]
struct PresenceAddressRow {
    address: String,
    last_seen_at: Option<String>,
    last_seen_source: Option<String>,
}

/// 保留清理：刪除早於保留期的 `observation_event`，回傳刪除筆數（見票 04、
/// ADR-0016）。
///
/// - 界線：`cutoff = now - retention_days`；刪除條件為 `observed_at < cutoff`
///   （嚴格早於），**恰在 cutoff 的事件保留**。
/// - 只動事件：`ip_presence` 現況與宣告資料（指派／保留）完全不變。
/// - 以 `idx_observation_event_observed` 索引掃描；清理為全站、不分網段。
pub async fn cleanup_events(
    pool: &SqlitePool,
    retention_days: u32,
    now: DateTime<Utc>,
) -> Result<u64, ApiError> {
    let cutoff = now
        .checked_sub_signed(chrono::Duration::days(i64::from(retention_days)))
        .ok_or_else(|| ApiError::internal("換算觀測事件保留期限失敗", retention_days))?;

    let result = sqlx::query("DELETE FROM observation_event WHERE observed_at < ?")
        .bind(timestamp(cutoff))
        .execute(pool)
        .await
        .map_err(|error| ApiError::internal("清理觀測事件失敗", error))?;

    Ok(result.rows_affected())
}

/// 一筆目前有效租約的觀測訊號（見 [`lease_signals`]）。
#[derive(Debug, Clone, PartialEq, Eq)]
struct LeaseSignal {
    address: Ipv4Addr,
    /// 正規化小寫 MAC；Kea 未提供或格式異常時為 `None`（不寫事件）。
    mac: Option<String>,
    /// `cltt` 轉 UTC；缺漏或超出可表示範圍時為 `None`（不記「最後可見」）。
    observed_at: Option<DateTime<Utc>>,
}

/// 讀取 Kea 目前有效租約並篩出本網段的觀測訊號。
///
/// 未設定 Kea（`None`）或本網段無 `kea_subnet_id` 時回空集合；讀取失敗只記
/// 警告、回空集合，不讓週期任務失敗（見 ADR-0011 非阻塞精神）。
async fn lease_signals(kea: Option<&KeaClient>, subnet: &Subnet) -> Vec<LeaseSignal> {
    let (Some(client), Some(kea_subnet_id)) = (kea, subnet.kea_subnet_id) else {
        return Vec::new();
    };

    match client.lease4_get_all().await {
        Ok(leases) => filter_lease_signals(&leases, kea_subnet_id),
        Err(error) => {
            tracing::warn!(
                subnet_id = subnet.id,
                %error,
                "讀取 Kea 租約失敗，本次略過該網段的租約觀測"
            );
            Vec::new()
        }
    }
}

/// 由 `lease4-get-all` 的租約挑出本網段目前有效的訊號（純函式；見票 03）。
///
/// 有效＝`state == Some("default")` ∧ `subnet_id == kea_subnet_id` ∧
/// `ip_address` 可解析為 IPv4；`hw-address` 經 MAC 正規化、異常視為未提供。
fn filter_lease_signals(leases: &[KeaLease], kea_subnet_id: i64) -> Vec<LeaseSignal> {
    leases
        .iter()
        .filter(|lease| {
            lease.state.as_deref() == Some("default") && lease.subnet_id == Some(kea_subnet_id)
        })
        .filter_map(|lease| {
            let address: Ipv4Addr = lease.ip_address.as_deref()?.parse().ok()?;
            Some(LeaseSignal {
                address,
                mac: lease.hw_address.as_deref().and_then(normalize_mac),
                observed_at: lease
                    .cltt
                    .and_then(|seconds| DateTime::from_timestamp(seconds, 0)),
            })
        })
        .collect()
}

/// 週期性 Kea 租約觀測（見票 08、spec §後端移除範圍）：讀取各受管 v4 網段的
/// 目前有效租約，以 `cltt` 記為最後可見（來源 `kea_lease`）；不做任何探測。
///
/// - 只處理有 `kea_subnet_id` 的 v4 網段；未設定 Kea 時不發送任何命令。
/// - 讀取失敗只記警告、略過該網段（不讓週期任務失敗；見 ADR-0011 精神）。
/// - 缺 `cltt` 的租約不記（無可信觀測時間）；缺 MAC 者仍記時間與來源。
/// - latest-wins 由 [`record_seen`] 處理：較舊的 `cltt` 不覆寫較新的現況。
/// - 回傳實際寫入的租約訊號數（供日誌）。
pub async fn record_lease_observations(
    pool: &SqlitePool,
    kea: Option<&KeaClient>,
) -> Result<u64, ApiError> {
    if kea.is_none() {
        return Ok(0);
    }

    let subnets = crate::subnets::list_full(pool)
        .await
        .map_err(|error| ApiError::internal("讀取網段清單失敗", error))?;

    // 先讀完所有租約（HTTP）再開交易，避免在網路 I/O 期間持有寫鎖。
    let mut pending: Vec<(i64, LeaseSignal)> = Vec::new();
    for subnet in subnets
        .iter()
        .filter(|subnet| subnet.kea_subnet_id.is_some() && !subnet.cidr.contains(':'))
    {
        for signal in lease_signals(kea, subnet).await {
            pending.push((subnet.id, signal));
        }
    }

    if pending.is_empty() {
        return Ok(0);
    }

    let mut transaction = pool
        .begin()
        .await
        .map_err(|error| ApiError::internal("建立租約觀測交易失敗", error))?;

    let mut recorded = 0u64;
    for (subnet_id, signal) in pending {
        let Some(observed_at) = signal.observed_at else {
            continue;
        };
        record_seen(
            &mut transaction,
            subnet_id,
            &signal.address.to_string(),
            signal.mac.as_deref(),
            "kea_lease",
            &timestamp(observed_at),
        )
        .await
        .map_err(|error| ApiError::internal("寫入租約觀測失敗", error))?;
        recorded += 1;
    }

    transaction
        .commit()
        .await
        .map_err(|error| ApiError::internal("提交租約觀測交易失敗", error))?;

    Ok(recorded)
}

/// upsert 目標的 `last_checked_at`（latest-wins：僅在觀測不舊於現況時更新）；
/// 既有 `last_seen_*` 不受影響。
pub(crate) async fn upsert_checked(
    connection: &mut SqliteConnection,
    subnet_id: i64,
    address: &str,
    checked_at: &str,
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO ip_presence (subnet_id, address, last_checked_at)
         VALUES (?, ?, ?)
         ON CONFLICT (subnet_id, address)
             DO UPDATE SET last_checked_at = excluded.last_checked_at
             WHERE ip_presence.last_checked_at IS NULL
                OR excluded.last_checked_at >= ip_presence.last_checked_at",
    )
    .bind(subnet_id)
    .bind(address)
    .bind(checked_at)
    .execute(&mut *connection)
    .await?;
    Ok(())
}

/// 現況轉移 → 事件種類：首次看到 `first_seen`、換 MAC `mac_changed`、
/// 同 MAC（不分大小寫）不寫事件（見 ADR-0016）。
pub(crate) fn transition_event(previous_mac: Option<&str>, new_mac: &str) -> Option<&'static str> {
    match previous_mac {
        None => Some("first_seen"),
        Some(previous) if !previous.eq_ignore_ascii_case(new_mac) => Some("mac_changed"),
        Some(_) => None,
    }
}

/// 寫入單筆「看到」：依現況轉移寫事件（append-only），latest-wins 更新現況。
///
/// `observed_at` 較舊（如 Kea `cltt`）時不覆寫較新的現況，亦不寫事件：
/// 事件只記錄現況（`last_seen_mac`）的實際轉移，否則舊訊號會在每輪回報
/// 以回溯時間重複寫入。時間相同（`>=`）視為不舊，仍更新現況與事件；
/// 寫入順序為租約先、代理回報後，同秒由代理主動觀測（`arp`）勝出。
///
/// `mac` 為 `None`（Kea 租約缺 `hw-address`）時不寫事件、亦不改寫既有
/// `last_seen_mac`（無證據不得否定既有 MAC），只更新時間與來源。
///
/// Kea 租約來源（[`record_lease_observations`]）以同一介面併入：來源帶
/// `kea_lease`、時間帶 `cltt`。
///
/// 此路徑**不觸碰** `out_of_subnet`（見票 01、ADR-0017）；被動監聽請用
/// [`record_passive_seen`]。
pub(crate) async fn record_seen(
    connection: &mut SqliteConnection,
    subnet_id: i64,
    address: &str,
    mac: Option<&str>,
    source: &str,
    observed_at: &str,
) -> sqlx::Result<()> {
    record_observation(
        connection,
        subnet_id,
        address,
        mac,
        source,
        observed_at,
        false,
    )
    .await
}

/// 寫入單筆被動監聽「看到」：比照 [`record_seen`]（事件、latest-wins 規則
/// 相同），並在現況列寫 `out_of_subnet = 1`。
///
/// 只有被動監聽路徑寫此欄；其他來源不觸碰，避免同一列在來源間翻轉
/// （見票 01、ADR-0017）。CIDR 過濾由呼叫端（代理入庫
/// [`crate::agents::record_observations`]）完成。
pub(crate) async fn record_passive_seen(
    connection: &mut SqliteConnection,
    subnet_id: i64,
    address: &str,
    mac: &str,
    observed_at: &str,
) -> sqlx::Result<()> {
    record_observation(
        connection,
        subnet_id,
        address,
        Some(mac),
        "arp_passive",
        observed_at,
        true,
    )
    .await
}

/// [`record_seen`]／[`record_passive_seen`] 的共用實作；`out_of_subnet`
/// 僅被動路徑為 `true`。
async fn record_observation(
    connection: &mut SqliteConnection,
    subnet_id: i64,
    address: &str,
    mac: Option<&str>,
    source: &str,
    observed_at: &str,
    out_of_subnet: bool,
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

    // latest-wins：僅在觀測不舊於現況時視為「現在的狀態」（票 03 租約 cltt
    // 同路徑）；較舊的訊號也不得寫事件，避免重複回溯。
    let newer = current.as_ref().is_none_or(|(previous_at, _)| {
        previous_at
            .as_deref()
            .is_none_or(|previous| observed_at >= previous)
    });

    if newer
        && let Some(mac) = mac
        && let Some(kind) = transition_event(previous_mac, mac)
    {
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

    if newer {
        match current {
            Some((_, previous_mac)) => {
                // MAC 未提供時保留既有值（無證據不得改寫）。
                let stored_mac = mac.or(previous_mac.as_deref());
                if out_of_subnet {
                    sqlx::query(
                        "UPDATE ip_presence
                            SET last_seen_at = ?, last_seen_mac = ?, last_seen_source = ?,
                                out_of_subnet = 1
                          WHERE subnet_id = ? AND address = ?",
                    )
                    .bind(observed_at)
                    .bind(stored_mac)
                    .bind(source)
                    .bind(subnet_id)
                    .bind(address)
                    .execute(&mut *connection)
                    .await?;
                } else {
                    sqlx::query(
                        "UPDATE ip_presence
                            SET last_seen_at = ?, last_seen_mac = ?, last_seen_source = ?
                          WHERE subnet_id = ? AND address = ?",
                    )
                    .bind(observed_at)
                    .bind(stored_mac)
                    .bind(source)
                    .bind(subnet_id)
                    .bind(address)
                    .execute(&mut *connection)
                    .await?;
                }
            }
            None => {
                if out_of_subnet {
                    sqlx::query(
                        "INSERT INTO ip_presence
                             (subnet_id, address, last_seen_at, last_seen_mac, last_seen_source,
                              out_of_subnet)
                         VALUES (?, ?, ?, ?, ?, 1)",
                    )
                    .bind(subnet_id)
                    .bind(address)
                    .bind(observed_at)
                    .bind(mac)
                    .bind(source)
                    .execute(&mut *connection)
                    .await?;
                } else {
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

    #[tokio::test]
    async fn passive_record_sets_flag_and_regular_path_never_touches_it() {
        let pool = test_pool().await;
        let subnet_id = insert_subnet(&pool, "10.0.0.0/29").await;
        let mut transaction = pool.begin().await.expect("建立交易");

        record_passive_seen(
            &mut transaction,
            subnet_id,
            "10.0.9.9",
            "aa:bb:cc:dd:ee:99",
            "2026-10-06T12:00:00Z",
        )
        .await
        .expect("寫入被動觀測");
        record_seen(
            &mut transaction,
            subnet_id,
            "10.0.0.5",
            Some("aa:bb:cc:dd:ee:05"),
            "arp",
            "2026-10-06T12:00:00Z",
        )
        .await
        .expect("寫入一般觀測");
        // 一般路徑即使命中同一列（理論上不會，因 CIDR 過濾）也不得清旗標。
        record_seen(
            &mut transaction,
            subnet_id,
            "10.0.9.9",
            Some("aa:bb:cc:dd:ee:99"),
            "arp",
            "2026-10-06T12:00:01Z",
        )
        .await
        .expect("一般觀測命中被動列");
        transaction.commit().await.expect("提交交易");

        let passive_flag: i64 =
            sqlx::query_scalar("SELECT out_of_subnet FROM ip_presence WHERE address = '10.0.9.9'")
                .fetch_one(&pool)
                .await
                .expect("讀取被動列旗標");
        assert_eq!(passive_flag, 1, "被動路徑寫 1 且不被後續一般路徑清除");

        let active_flag: i64 =
            sqlx::query_scalar("SELECT out_of_subnet FROM ip_presence WHERE address = '10.0.0.5'")
                .fetch_one(&pool)
                .await
                .expect("讀取一般列旗標");
        assert_eq!(active_flag, 0, "一般路徑不觸碰旗標（維持預設 0）");

        let events = sqlx::query_as::<_, (String, String, String)>(
            "SELECT address, kind, source FROM observation_event ORDER BY id ASC",
        )
        .fetch_all(&pool)
        .await
        .expect("讀取事件");
        assert_eq!(
            events,
            vec![
                (
                    "10.0.9.9".to_string(),
                    "first_seen".to_string(),
                    "arp_passive".to_string()
                ),
                (
                    "10.0.0.5".to_string(),
                    "first_seen".to_string(),
                    "arp".to_string()
                ),
            ],
            "被動列首見來源 arp_passive；一般列同 MAC 不再寫事件"
        );
    }

    #[test]
    fn normalize_mac_accepts_colons_and_dashes_and_lowercases() {
        assert_eq!(
            normalize_mac("AA:BB:CC:dd:ee:ff"),
            Some("aa:bb:cc:dd:ee:ff".to_string())
        );
        assert_eq!(
            normalize_mac("AA-BB-CC-DD-EE-FF\n"),
            Some("aa:bb:cc:dd:ee:ff".to_string()),
            "連字號與前後空白正規化"
        );
        assert_eq!(
            normalize_mac("AA:BB:CC:DD:EE:FF:00:11"),
            Some("aa:bb:cc:dd:ee:ff:00:11".to_string()),
            "EUI-64（8 組）亦接受"
        );
        assert_eq!(normalize_mac("aa:bb:cc:dd:ee"), None, "群組數不足");
        assert_eq!(normalize_mac("aa:bb:cc:dd:ee:g1"), None, "非十六進位");
        assert_eq!(normalize_mac(""), None);
    }

    #[test]
    fn timestamp_uses_spec_utc_format() {
        let now = DateTime::parse_from_rfc3339("2026-10-06T12:34:56Z")
            .expect("固定時間")
            .with_timezone(&Utc);
        assert_eq!(timestamp(now), "2026-10-06T12:34:56Z");
    }

    /// 建立一筆 Kea 租約（未指定的欄位為 `None`）。
    fn lease(
        state: Option<&str>,
        subnet_id: Option<i64>,
        ip_address: Option<&str>,
        hw_address: Option<&str>,
        cltt: Option<i64>,
    ) -> KeaLease {
        KeaLease {
            ip_address: ip_address.map(str::to_string),
            hw_address: hw_address.map(str::to_string),
            hostname: None,
            subnet_id,
            cltt,
            valid_lft: None,
            state: state.map(str::to_string),
        }
    }

    #[test]
    fn filter_lease_signals_keeps_only_default_leases_of_matching_subnet() {
        let leases = vec![
            lease(
                Some("default"),
                Some(7),
                Some("10.0.0.5"),
                Some("AA:BB:CC:DD:EE:05"),
                Some(1_791_201_600),
            ),
            lease(Some("declined"), Some(7), Some("10.0.0.6"), None, Some(0)),
            lease(Some("expired"), Some(7), Some("10.0.0.7"), None, Some(0)),
            lease(Some("released"), Some(7), Some("10.0.0.8"), None, Some(0)),
            lease(None, Some(7), Some("10.0.0.9"), None, Some(0)),
            lease(Some("default"), Some(8), Some("10.0.0.10"), None, Some(0)),
            lease(Some("default"), Some(7), Some("fd42::5"), None, Some(0)),
            lease(Some("default"), Some(7), None, None, Some(0)),
            // 缺 cltt 與 MAC 格式異常：仍納入目標，但時間／MAC 為 None。
            lease(
                Some("default"),
                Some(7),
                Some("10.0.0.11"),
                Some("not-a-mac"),
                None,
            ),
        ];

        let signals = filter_lease_signals(&leases, 7);

        assert_eq!(
            signals
                .iter()
                .map(|signal| signal.address)
                .collect::<Vec<_>>(),
            vec![
                "10.0.0.5".parse::<Ipv4Addr>().expect("位址一"),
                "10.0.0.11".parse::<Ipv4Addr>().expect("位址二"),
            ],
            "只收 state=default、subnet 對應、IPv4 可解析者"
        );
        assert_eq!(
            signals[0].mac.as_deref(),
            Some("aa:bb:cc:dd:ee:05"),
            "MAC 正規化為小寫冒號格式"
        );
        assert_eq!(
            signals[0].observed_at.as_ref().map(|at| timestamp(*at)),
            Some("2026-10-05T12:00:00Z".to_string()),
            "cltt（epoch 秒）轉 UTC"
        );
        assert_eq!(signals[1].mac, None, "MAC 格式異常視為未提供");
        assert_eq!(signals[1].observed_at, None, "缺 cltt 無最後可見時間");
    }

    /// 建立測試資料庫並套用 migrations（比照 `backend/tests/` 整合測試）。
    async fn test_pool() -> SqlitePool {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
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

    /// 植入測試網段，回傳 id。
    async fn insert_subnet(pool: &SqlitePool, cidr: &str) -> i64 {
        sqlx::query("INSERT INTO subnets (cidr) VALUES (?)")
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

    #[tokio::test]
    async fn cleanup_keeps_event_exactly_at_cutoff_and_deletes_older() {
        let pool = test_pool().await;
        let subnet_id = insert_subnet(&pool, "10.0.0.0/29").await;
        let now = DateTime::parse_from_rfc3339("2026-10-06T00:00:00Z")
            .expect("固定時間")
            .with_timezone(&Utc);
        // cutoff = 2025-10-06T00:00:00Z（365 天前；2026 非閏年）。
        insert_event(&pool, subnet_id, "10.0.0.1", "2025-10-05T23:59:59Z").await;
        insert_event(&pool, subnet_id, "10.0.0.2", "2025-10-06T00:00:00Z").await;
        insert_event(&pool, subnet_id, "10.0.0.3", "2026-10-05T23:59:59Z").await;

        let deleted = cleanup_events(&pool, 365, now).await.expect("清理成功");

        assert_eq!(deleted, 1, "只刪嚴格早於 cutoff 者");
        let remaining: Vec<String> =
            sqlx::query_scalar("SELECT address FROM observation_event ORDER BY address")
                .fetch_all(&pool)
                .await
                .expect("讀取剩餘事件");
        assert_eq!(
            remaining,
            ["10.0.0.2".to_string(), "10.0.0.3".to_string()],
            "恰在 cutoff 的事件保留（observed_at >= cutoff）"
        );
    }

    #[tokio::test]
    async fn cleanup_leaves_presence_untouched() {
        let pool = test_pool().await;
        let subnet_id = insert_subnet(&pool, "10.0.0.0/29").await;
        insert_event(&pool, subnet_id, "10.0.0.1", "2020-01-01T00:00:00Z").await;
        sqlx::query(
            "INSERT INTO ip_presence
                 (subnet_id, address, last_seen_at, last_seen_mac, last_seen_source, last_checked_at)
             VALUES (?, '10.0.0.1', '2020-01-01T00:00:00Z', 'aa:bb:cc:dd:ee:ff', 'arp',
                     '2026-10-06T00:00:00Z')",
        )
        .bind(subnet_id)
        .execute(&pool)
        .await
        .expect("植入現況");

        let now = DateTime::parse_from_rfc3339("2026-10-06T00:00:00Z")
            .expect("固定時間")
            .with_timezone(&Utc);
        let deleted = cleanup_events(&pool, 365, now).await.expect("清理成功");

        assert_eq!(deleted, 1);
        let presence: Option<String> = sqlx::query_scalar("SELECT last_seen_mac FROM ip_presence")
            .fetch_optional(&pool)
            .await
            .expect("讀取現況");
        assert_eq!(
            presence.as_deref(),
            Some("aa:bb:cc:dd:ee:ff"),
            "現況列不受清理影響"
        );
    }
}
