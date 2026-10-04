//! 網段（Subnet）領域模組：CIDR 正規化、結構驗證與資料庫存取。
//!
//! 詞彙依 `CONTEXT.md`；規則見 `.scratch/asset-ip-management/spec.md` §2.3、§3.1。
//! 單一網段為單一地址族，雙棧以兩筆表示；pool 與 kea_subnet_id 僅支援 IPv4。

use std::net::{IpAddr, Ipv4Addr};

use ipnet::IpNet;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{FromRow, SqlitePool};

use crate::api::ApiError;
use crate::assets::{double_option, optional_text};
use crate::assignments;
use crate::conflicts;
use crate::ips::HostRange;

/// `subnets` 資料表完整欄位清單。
const COLUMNS: &str = "id, cidr, name, note, gateway, kea_subnet_id, created_at, updated_at";

/// `subnets` 資料表列。
#[derive(Debug, FromRow)]
struct SubnetRow {
    id: i64,
    cidr: String,
    name: Option<String>,
    note: Option<String>,
    gateway: Option<String>,
    kea_subnet_id: Option<i64>,
    created_at: String,
    updated_at: String,
}

/// `subnet_pools` 資料表列。
#[derive(Debug, FromRow)]
struct PoolRow {
    id: i64,
    start_ip: String,
    end_ip: String,
}

/// API 回傳的 DHCP 位址池（僅 IPv4；見 spec §2.3）。
#[derive(Debug, Serialize)]
pub struct Pool {
    pub id: i64,
    pub start_ip: String,
    pub end_ip: String,
}

/// API 回傳的網段（含 pools）。
#[derive(Debug, Serialize)]
pub struct Subnet {
    pub id: i64,
    /// 正規化 CIDR：host bits 收斂為網路地址。
    pub cidr: String,
    pub name: Option<String>,
    pub note: Option<String>,
    pub gateway: Option<String>,
    pub kea_subnet_id: Option<i64>,
    pub pools: Vec<Pool>,
    pub created_at: String,
    pub updated_at: String,
}

/// 列表摘要：名稱、CIDR、地址族與統計（見 spec §2.3、票 07）。
#[derive(Debug, Serialize)]
pub struct SubnetSummary {
    pub id: i64,
    pub cidr: String,
    pub name: Option<String>,
    /// `ipv4` 或 `ipv6`。
    pub family: &'static str,
    /// 已用：static＋reservation 指派數（v6 即已登錄數）。
    pub used: u64,
    /// 總數：v4 為 host 數（扣 network/broadcast；`/31`、`/32` 全列）；
    /// v6 為已登錄數（= `used`，UI 顯示為「已登錄 N」）。
    pub total: u64,
    /// 衝突數：命中至少一條語意規則的指派筆數（同一筆命中多條規則仍計 1；
    /// 見 [`crate::conflicts::detect`]）。
    pub conflicts: u64,
}

/// 新增網段的 pool 輸入。
#[derive(Debug, Default, Deserialize)]
pub struct PoolInput {
    pub start_ip: Option<String>,
    pub end_ip: Option<String>,
}

/// 新增網段的輸入；缺漏欄位視為未填。
#[derive(Debug, Default, Deserialize)]
pub struct SubnetInput {
    pub cidr: Option<String>,
    pub name: Option<String>,
    pub note: Option<String>,
    pub gateway: Option<String>,
    pub kea_subnet_id: Option<i64>,
    #[serde(default)]
    pub pools: Vec<PoolInput>,
}

/// 編輯網段的輸入。
///
/// 外層 `None`＝欄位未提供（維持原值）；`Some(None)`＝顯式 `null`（清除）；
/// `Some(Some(..))`＝設定新值。`pools` 提供時整批取代（空陣列＝清空）。
#[derive(Debug, Default, Deserialize)]
pub struct SubnetPatch {
    pub cidr: Option<String>,
    #[serde(default, deserialize_with = "double_option")]
    pub name: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub note: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub gateway: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub kea_subnet_id: Option<Option<i64>>,
    pub pools: Option<Vec<PoolInput>>,
}

/// 已驗證的網段內容。
#[derive(Debug)]
pub struct ValidSubnet {
    cidr: IpNet,
    name: Option<String>,
    note: Option<String>,
    gateway: Option<IpAddr>,
    kea_subnet_id: Option<i64>,
    pools: Vec<ValidPool>,
}

/// 已驗證的 pool 範圍（僅 IPv4）。
#[derive(Debug, Clone, Copy)]
struct ValidPool {
    start: Ipv4Addr,
    end: Ipv4Addr,
}

impl SubnetInput {
    /// 驗證新增欄位；結構錯誤阻擋儲存（見 ADR-0006）。
    pub fn validate(self) -> Result<ValidSubnet, ApiError> {
        validate_fields(
            self.cidr,
            self.name,
            self.note,
            self.gateway,
            self.kea_subnet_id,
            self.pools,
        )
    }
}

impl SubnetPatch {
    /// 與既有網段合併後驗證：以合併後的最終狀態判斷結構規則。
    pub fn apply_to(self, existing: &Subnet) -> Result<ValidSubnet, ApiError> {
        let pools = match self.pools {
            Some(pools) => pools,
            None => existing
                .pools
                .iter()
                .map(|pool| PoolInput {
                    start_ip: Some(pool.start_ip.clone()),
                    end_ip: Some(pool.end_ip.clone()),
                })
                .collect(),
        };

        validate_fields(
            Some(self.cidr.unwrap_or_else(|| existing.cidr.clone())),
            self.name.unwrap_or_else(|| existing.name.clone()),
            self.note.unwrap_or_else(|| existing.note.clone()),
            self.gateway.unwrap_or_else(|| existing.gateway.clone()),
            self.kea_subnet_id.unwrap_or(existing.kea_subnet_id),
            pools,
        )
    }
}

/// 驗證並正規化網段欄位；結構錯誤阻擋儲存（見 spec §3.1）。
fn validate_fields(
    cidr: Option<String>,
    name: Option<String>,
    note: Option<String>,
    gateway: Option<String>,
    kea_subnet_id: Option<i64>,
    pools: Vec<PoolInput>,
) -> Result<ValidSubnet, ApiError> {
    let cidr = normalize_cidr(cidr)?;
    let is_v4 = cidr.addr().is_ipv4();

    let gateway = match optional_text(gateway) {
        None => None,
        Some(text) => {
            let address: IpAddr = text.parse().map_err(|_| {
                ApiError::validation(format!("gateway 格式錯誤：{text}")).field("gateway")
            })?;
            if !cidr.contains(&address) {
                return Err(
                    ApiError::validation(format!("gateway {address} 不在網段 {cidr} 內"))
                        .field("gateway"),
                );
            }
            Some(address)
        }
    };

    if !is_v4 && kea_subnet_id.is_some() {
        return Err(ApiError::validation("IPv6 網段不支援 kea_subnet_id").field("kea_subnet_id"));
    }
    if !is_v4 && !pools.is_empty() {
        return Err(ApiError::validation("IPv6 網段不支援 pool").field("pools"));
    }

    let mut valid_pools = Vec::with_capacity(pools.len());
    for (index, pool) in pools.into_iter().enumerate() {
        let label = format!("pools[{index}]");
        let start = require_pool_address(pool.start_ip, &label, "start_ip")?;
        let end = require_pool_address(pool.end_ip, &label, "end_ip")?;

        if start > end {
            return Err(
                ApiError::validation(format!("{label} 起點 {start} 不可大於終點 {end}"))
                    .field(&format!("{label}.start_ip")),
            );
        }
        if !cidr.contains(&IpAddr::V4(start)) || !cidr.contains(&IpAddr::V4(end)) {
            return Err(ApiError::validation(format!(
                "{label} 範圍 {start}–{end} 不在網段 {cidr} 內"
            ))
            .field(&format!("{label}.start_ip")));
        }

        valid_pools.push(ValidPool { start, end });
    }

    for (index, pool) in valid_pools.iter().enumerate() {
        for (other_index, other) in valid_pools.iter().enumerate().skip(index + 1) {
            if pool.overlaps(other) {
                return Err(ApiError::validation(format!(
                    "pools[{index}]（{}–{}）與 pools[{other_index}]（{}–{}）重疊",
                    pool.start, pool.end, other.start, other.end
                ))
                .field("pools"));
            }
        }
    }

    Ok(ValidSubnet {
        cidr,
        name: optional_text(name),
        note: optional_text(note),
        gateway,
        kea_subnet_id,
        pools: valid_pools,
    })
}

impl ValidPool {
    /// 兩段 pool 是否重疊（端點皆含）。
    fn overlaps(&self, other: &ValidPool) -> bool {
        self.start <= other.end && other.start <= self.end
    }
}

/// CIDR：必填、須為合法 CIDR；host bits 收斂為網路地址後儲存。
fn normalize_cidr(value: Option<String>) -> Result<IpNet, ApiError> {
    let text =
        optional_text(value).ok_or_else(|| ApiError::validation("CIDR 為必填").field("cidr"))?;
    let network: IpNet = text.parse().map_err(|_| {
        ApiError::validation(format!("CIDR 格式錯誤：{text}（例：192.168.1.0/24）")).field("cidr")
    })?;
    Ok(network.trunc())
}

/// pool 端點：必填且須為 IPv4 位址。
fn require_pool_address(
    value: Option<String>,
    label: &str,
    key: &str,
) -> Result<Ipv4Addr, ApiError> {
    let field = format!("{label}.{key}");
    let text = optional_text(value)
        .ok_or_else(|| ApiError::validation(format!("{label} 的 {key} 為必填")).field(&field))?;
    text.parse()
        .map_err(|_| ApiError::validation(format!("pool 位址格式錯誤：{text}")).field(&field))
}

/// 結構衝突檢查：與既有網段重疊（含完全相同、嵌套）、kea_subnet_id 重複。
///
/// `exclude_id`＝編輯時排除自身。結構錯誤阻擋儲存（見 ADR-0006）。
pub async fn ensure_no_conflicts(
    pool: &SqlitePool,
    valid: &ValidSubnet,
    exclude_id: Option<i64>,
) -> Result<(), ApiError> {
    let rows: Vec<(i64, String, Option<String>, Option<i64>)> = sqlx::query_as(
        "SELECT id, cidr, name, kea_subnet_id FROM subnets WHERE (? IS NULL OR id <> ?)",
    )
    .bind(exclude_id)
    .bind(exclude_id)
    .fetch_all(pool)
    .await
    .map_err(|error| ApiError::internal("讀取既有網段失敗", error))?;

    for (id, cidr, name, kea_subnet_id) in rows {
        let other: IpNet = cidr
            .parse()
            .map_err(|error| ApiError::internal("既有網段 CIDR 格式錯誤", error))?;

        // CIDR 區塊具層級性：任兩塊合法 CIDR 非互斥即互相包含。
        if valid.cidr.contains(&other) || other.contains(&valid.cidr) {
            return Err(ApiError::validation(format!(
                "與既有網段{}重疊（網段不得重疊，含嵌套）",
                describe(&cidr, name.as_deref())
            ))
            .field("cidr")
            .detail("conflict", json!({ "id": id, "cidr": cidr, "name": name })));
        }

        if let (Some(new_id), Some(existing_id)) = (valid.kea_subnet_id, kea_subnet_id) {
            if new_id == existing_id {
                return Err(ApiError::validation(format!(
                    "kea_subnet_id {new_id} 已被既有網段{}使用",
                    describe(&cidr, name.as_deref())
                ))
                .field("kea_subnet_id")
                .detail("conflict", json!({ "id": id, "cidr": cidr, "name": name })));
            }
        }
    }

    Ok(())
}

/// 衝突訊息中的網段描述：有名稱時附上名稱。
fn describe(cidr: &str, name: Option<&str>) -> String {
    match name {
        Some(name) if !name.is_empty() => format!("「{name}」（{cidr}）"),
        _ => format!(" {cidr} "),
    }
}

/// 網段清單：依建立順序（id 升冪），附已用／總數／衝突數統計（見票 07）。
///
/// 統計即時計算、無快取：逐網段讀取 pools 與指派並偵測衝突；網段編輯
/// （縮小 CIDR、擴大 pool）或指派異動後，下一次讀取即反映最新結果。
pub async fn list(pool: &SqlitePool) -> Result<Vec<SubnetSummary>, ApiError> {
    let rows =
        sqlx::query_as::<_, SubnetRow>(&format!("SELECT {COLUMNS} FROM subnets ORDER BY id ASC"))
            .fetch_all(pool)
            .await
            .map_err(|error| ApiError::internal("讀取網段清單失敗", error))?;

    let mut summaries = Vec::with_capacity(rows.len());
    for row in rows {
        let pools = fetch_pools(pool, row.id)
            .await
            .map_err(|error| ApiError::internal("讀取網段 pools 失敗", error))?;
        let subnet = row.into_subnet(pools);

        let assignments = assignments::list_for_subnet(pool, subnet.id)
            .await
            .map_err(|error| ApiError::internal("讀取指派清單失敗", error))?;
        let used = assignments.len() as u64;
        let conflicts = conflicts::detect(&subnet, &assignments)?.len() as u64;

        let family = family_of(&subnet.cidr);
        let total = match subnet.cidr.parse::<IpNet>() {
            Ok(IpNet::V4(network)) => HostRange::of(&network).count(),
            Ok(IpNet::V6(_)) => used,
            Err(error) => return Err(ApiError::internal("網段 CIDR 格式錯誤", error)),
        };

        summaries.push(SubnetSummary {
            id: subnet.id,
            cidr: subnet.cidr,
            name: subnet.name,
            family,
            used,
            total,
            conflicts,
        });
    }

    Ok(summaries)
}

/// 讀取單一網段（含 pools）；不存在回傳 `None`。
pub async fn get(pool: &SqlitePool, id: i64) -> sqlx::Result<Option<Subnet>> {
    let Some(row) = fetch_row(pool, id).await? else {
        return Ok(None);
    };
    let pools = fetch_pools(pool, id).await?;
    Ok(Some(row.into_subnet(pools)))
}

/// 依位址尋找所屬網段（含 pools）；找不到回傳 `None`。
///
/// 網段不得重疊（見 spec §3.1），故至多一個網段包含該位址。供資產端指派
/// 由位址反推網段（見票 10）；位址若因網段縮小而出界，將找不到所屬網段。
pub async fn find_by_address(
    pool: &SqlitePool,
    address: IpAddr,
) -> Result<Option<Subnet>, ApiError> {
    let rows =
        sqlx::query_as::<_, SubnetRow>(&format!("SELECT {COLUMNS} FROM subnets ORDER BY id ASC"))
            .fetch_all(pool)
            .await
            .map_err(|error| ApiError::internal("讀取網段失敗", error))?;

    for row in rows {
        let network: IpNet = row
            .cidr
            .parse()
            .map_err(|error| ApiError::internal("網段 CIDR 格式錯誤", error))?;
        if network.contains(&address) {
            let pools = fetch_pools(pool, row.id)
                .await
                .map_err(|error| ApiError::internal("讀取網段 pools 失敗", error))?;
            return Ok(Some(row.into_subnet(pools)));
        }
    }

    Ok(None)
}

/// 新增網段與其 pools（同一交易）。
pub async fn create(pool: &SqlitePool, valid: ValidSubnet) -> sqlx::Result<Subnet> {
    let mut transaction = pool.begin().await?;
    let result = sqlx::query(
        "INSERT INTO subnets (cidr, name, note, gateway, kea_subnet_id) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(valid.cidr.to_string())
    .bind(valid.name)
    .bind(valid.note)
    .bind(valid.gateway.map(|address| address.to_string()))
    .bind(valid.kea_subnet_id)
    .execute(&mut *transaction)
    .await?;

    let id = result.last_insert_rowid();
    insert_pools(&mut transaction, id, &valid.pools).await?;
    transaction.commit().await?;

    get(pool, id).await?.ok_or(sqlx::Error::RowNotFound)
}

/// 編輯網段；pools 一律以新內容整批取代。不存在回傳 `None`。
pub async fn update(
    pool: &SqlitePool,
    id: i64,
    valid: ValidSubnet,
) -> sqlx::Result<Option<Subnet>> {
    let mut transaction = pool.begin().await?;
    let result = sqlx::query(
        "UPDATE subnets
             SET cidr = ?, name = ?, note = ?, gateway = ?, kea_subnet_id = ?,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
           WHERE id = ?",
    )
    .bind(valid.cidr.to_string())
    .bind(valid.name)
    .bind(valid.note)
    .bind(valid.gateway.map(|address| address.to_string()))
    .bind(valid.kea_subnet_id)
    .bind(id)
    .execute(&mut *transaction)
    .await?;

    if result.rows_affected() == 0 {
        return Ok(None);
    }

    sqlx::query("DELETE FROM subnet_pools WHERE subnet_id = ?")
        .bind(id)
        .execute(&mut *transaction)
        .await?;
    insert_pools(&mut transaction, id, &valid.pools).await?;
    transaction.commit().await?;

    get(pool, id).await
}

/// 刪除前防護：有任何指派或保留（v4／v6）即不可刪除（見 spec §2.3、§3.1）。
///
/// 回應 409 `conflict`，`details.assignments` 附指派筆數（含保留），
/// 供前端顯示明確錯誤（見票 08）。
pub async fn ensure_deletable(pool: &SqlitePool, subnet: &Subnet) -> Result<(), ApiError> {
    let count = assignments::count_for_subnet(pool, subnet.id)
        .await
        .map_err(|error| ApiError::internal("讀取指派筆數失敗", error))?;

    if count > 0 {
        return Err(ApiError::conflict(format!(
            "網段{}尚有 {count} 筆指派（含保留），不可刪除；請先取消所有指派",
            describe(&subnet.cidr, subnet.name.as_deref())
        ))
        .detail("assignments", json!(count)));
    }

    Ok(())
}

/// 刪除網段；pools 連動刪除。有指派時呼叫端須先經 [`ensure_deletable`] 阻擋。
pub async fn delete(pool: &SqlitePool, id: i64) -> sqlx::Result<bool> {
    let result = sqlx::query("DELETE FROM subnets WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

/// 讀取一列；不存在回傳 `None`。
async fn fetch_row(pool: &SqlitePool, id: i64) -> sqlx::Result<Option<SubnetRow>> {
    sqlx::query_as::<_, SubnetRow>(&format!("SELECT {COLUMNS} FROM subnets WHERE id = ?"))
        .bind(id)
        .fetch_optional(pool)
        .await
}

/// 讀取某網段的 pools；依 id 升冪（即輸入順序）。
async fn fetch_pools(pool: &SqlitePool, subnet_id: i64) -> sqlx::Result<Vec<Pool>> {
    let rows = sqlx::query_as::<_, PoolRow>(
        "SELECT id, start_ip, end_ip FROM subnet_pools WHERE subnet_id = ? ORDER BY id ASC",
    )
    .bind(subnet_id)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| Pool {
            id: row.id,
            start_ip: row.start_ip,
            end_ip: row.end_ip,
        })
        .collect())
}

/// 寫入 pools；呼叫端負責交易。
async fn insert_pools(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    subnet_id: i64,
    pools: &[ValidPool],
) -> sqlx::Result<()> {
    for pool in pools {
        sqlx::query("INSERT INTO subnet_pools (subnet_id, start_ip, end_ip) VALUES (?, ?, ?)")
            .bind(subnet_id)
            .bind(pool.start.to_string())
            .bind(pool.end.to_string())
            .execute(&mut **transaction)
            .await?;
    }
    Ok(())
}

impl SubnetRow {
    fn into_subnet(self, pools: Vec<Pool>) -> Subnet {
        Subnet {
            id: self.id,
            cidr: self.cidr,
            name: self.name,
            note: self.note,
            gateway: self.gateway,
            kea_subnet_id: self.kea_subnet_id,
            pools,
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

/// 由 CIDR 文字判斷地址族；資料庫內容一律為已正規化的 CIDR。
fn family_of(cidr: &str) -> &'static str {
    if cidr.contains(':') { "ipv6" } else { "ipv4" }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 便捷呼叫：僅給 CIDR 的最小驗證。
    fn validate_cidr(cidr: &str) -> Result<ValidSubnet, ApiError> {
        validate_fields(Some(cidr.to_string()), None, None, None, None, vec![])
    }

    #[test]
    fn cidr_normalization_collapses_host_bits() {
        let valid = validate_cidr("192.168.1.5/24").expect("合法 CIDR");
        assert_eq!(valid.cidr.to_string(), "192.168.1.0/24");

        let valid = validate_cidr("fd00::1234/64").expect("合法 CIDR");
        assert_eq!(valid.cidr.to_string(), "fd00::/64");
    }

    #[test]
    fn cidr_is_required_and_validated() {
        assert!(validate_fields(None, None, None, None, None, vec![]).is_err());
        assert!(validate_cidr("192.168.1.0").is_err(), "缺前綴長度");
        assert!(validate_cidr("192.168.1.0/33").is_err(), "前綴超界");
        assert!(validate_cidr("not-a-cidr").is_err());
    }

    #[test]
    fn gateway_must_be_inside_cidr() {
        assert!(validate_cidr("10.0.0.0/24").is_ok(), "無 gateway");
        assert!(
            validate_fields(
                Some("10.0.0.0/24".to_string()),
                None,
                None,
                Some("10.0.0.1".to_string()),
                None,
                vec![],
            )
            .is_ok()
        );
        assert!(
            validate_fields(
                Some("10.0.0.0/24".to_string()),
                None,
                None,
                Some("10.0.1.1".to_string()),
                None,
                vec![],
            )
            .is_err(),
            "gateway 不在 CIDR 內"
        );
        assert!(
            validate_fields(
                Some("fd00::/64".to_string()),
                None,
                None,
                Some("10.0.0.1".to_string()),
                None,
                vec![],
            )
            .is_err(),
            "跨地址族的 gateway 不在 CIDR 內"
        );
    }

    #[test]
    fn v6_rejects_pools_and_kea_subnet_id() {
        assert!(
            validate_fields(
                Some("fd00::/64".to_string()),
                None,
                None,
                None,
                Some(1),
                vec![],
            )
            .is_err()
        );
        assert!(
            validate_fields(
                Some("fd00::/64".to_string()),
                None,
                None,
                None,
                None,
                vec![PoolInput {
                    start_ip: Some("fd00::10".to_string()),
                    end_ip: Some("fd00::20".to_string()),
                }],
            )
            .is_err()
        );
        assert!(
            validate_fields(
                Some("fd00::/64".to_string()),
                None,
                None,
                None,
                None,
                vec![],
            )
            .is_ok(),
            "v6 無 pool、無 kea_subnet_id 即可建立"
        );
    }

    #[test]
    fn pool_range_and_overlap_rules() {
        let pool = |start: &str, end: &str| PoolInput {
            start_ip: Some(start.to_string()),
            end_ip: Some(end.to_string()),
        };

        assert!(
            validate_fields(
                Some("10.0.0.0/24".to_string()),
                None,
                None,
                None,
                None,
                vec![pool("10.0.0.10", "10.0.0.20")],
            )
            .is_ok()
        );
        assert!(
            validate_fields(
                Some("10.0.0.0/24".to_string()),
                None,
                None,
                None,
                None,
                vec![pool("10.0.1.10", "10.0.1.20")],
            )
            .is_err(),
            "pool 不在 CIDR 內"
        );
        assert!(
            validate_fields(
                Some("10.0.0.0/24".to_string()),
                None,
                None,
                None,
                None,
                vec![pool("10.0.0.20", "10.0.0.10")],
            )
            .is_err(),
            "起點大於終點"
        );
        assert!(
            validate_fields(
                Some("10.0.0.0/24".to_string()),
                None,
                None,
                None,
                None,
                vec![
                    pool("10.0.0.10", "10.0.0.20"),
                    pool("10.0.0.20", "10.0.0.30")
                ],
            )
            .is_err(),
            "共用端點視為重疊"
        );
        assert!(
            validate_fields(
                Some("10.0.0.0/24".to_string()),
                None,
                None,
                None,
                None,
                vec![
                    pool("10.0.0.10", "10.0.0.19"),
                    pool("10.0.0.20", "10.0.0.30")
                ],
            )
            .is_ok(),
            "相鄰不重疊"
        );
    }

    #[test]
    fn family_detection_uses_address_text() {
        assert_eq!(family_of("10.0.0.0/8"), "ipv4");
        assert_eq!(family_of("fd00::/64"), "ipv6");
    }
}
