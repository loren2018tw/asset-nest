//! 網段（Subnet）領域模組：CIDR 正規化、結構驗證與資料庫存取。
//!
//! 詞彙依 `GLOSSARY.md`；規則見 `.scratch/asset-ip-management/spec.md` §2.3、§3.1。
//! 單一網段為單一地址族，雙棧以兩筆表示；pool 與 kea_subnet_id 僅支援 IPv4。

use std::cmp::Ordering;
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr};

use ipnet::IpNet;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{FromRow, SqliteConnection, SqlitePool};

use crate::api::ApiError;
use crate::assets::{double_option, optional_text};
use crate::assignments;
use crate::conflicts;
use crate::ips::HostRange;
use crate::probe::Prober;

/// `subnets` 資料表完整欄位清單。
const COLUMNS: &str =
    "id, cidr, name, note, gateway, kea_subnet_id, observed, created_at, updated_at";

/// `subnets` 資料表列。
#[derive(Debug, FromRow)]
struct SubnetRow {
    id: i64,
    cidr: String,
    name: Option<String>,
    note: Option<String>,
    gateway: Option<String>,
    kea_subnet_id: Option<i64>,
    observed: i64,
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

/// 一次讀取全部 pools 用的列；附所屬網段 id（匯出避免 N+1）。
#[derive(Debug, FromRow)]
struct PoolJoinRow {
    subnet_id: i64,
    id: i64,
    start_ip: String,
    end_ip: String,
}

/// API 回傳的 DHCP 位址池（僅 IPv4；見 spec §2.3）。
///
/// `Clone`：掃描服務以 [`crate::observation::run_quick`] 複製網段至
/// `spawn_blocking` 的探測工作。
#[derive(Debug, Clone, Serialize)]
pub struct Pool {
    pub id: i64,
    pub start_ip: String,
    pub end_ip: String,
}

/// API 回傳的網段（含 pools）。
#[derive(Debug, Clone, Serialize)]
pub struct Subnet {
    pub id: i64,
    /// 正規化 CIDR：host bits 收斂為網路地址。
    pub cidr: String,
    pub name: Option<String>,
    pub note: Option<String>,
    pub gateway: Option<String>,
    pub kea_subnet_id: Option<i64>,
    /// 觀測開關（快速掃描；見 ADR-0014）；v6 網段恆為 false。
    pub observed: bool,
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
    /// 觀測開關（見 ADR-0014）；v6 恆為 false。
    pub observed: bool,
    /// 本機是否有介面位址落在該 v4 子網（同 L2；由注入的探測邊界判定）。
    pub local: bool,
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
    /// 觀測開關（見票 01、ADR-0014）；未提供＝維持原值，v6 不得為 true。
    pub observed: Option<bool>,
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
    observed: bool,
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
        let observed = self.observed.unwrap_or(existing.observed);
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

        let mut valid = validate_fields(
            Some(self.cidr.unwrap_or_else(|| existing.cidr.clone())),
            self.name.unwrap_or_else(|| existing.name.clone()),
            self.note.unwrap_or_else(|| existing.note.clone()),
            self.gateway.unwrap_or_else(|| existing.gateway.clone()),
            self.kea_subnet_id.unwrap_or(existing.kea_subnet_id),
            pools,
        )?;

        // 結構驗證：v6 網段不得開啟觀測（關閉或 v4 任何值皆可；見票 01）。
        if observed && !valid.cidr.addr().is_ipv4() {
            return Err(ApiError::validation("IPv6 網段不支援觀測").field("observed"));
        }
        valid.observed = observed;

        Ok(valid)
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
        observed: false,
    })
}

impl ValidPool {
    /// 兩段 pool 是否重疊（端點皆含）。
    fn overlaps(&self, other: &ValidPool) -> bool {
        self.start <= other.end && other.start <= self.end
    }
}

/// CIDR：必填、須為合法 CIDR；host bits 收斂為網路地址後儲存。
///
/// 供匯入列級驗證重用（正規化與地址族判斷；見票 02）。
pub(crate) fn normalize_cidr(value: Option<String>) -> Result<IpNet, ApiError> {
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
/// `local` 以注入的探測邊界判定（見票 01、ADR-0015）。
pub async fn list(pool: &SqlitePool, prober: &dyn Prober) -> Result<Vec<SubnetSummary>, ApiError> {
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
        let local = prober.is_local(&subnet);

        summaries.push(SubnetSummary {
            id: subnet.id,
            cidr: subnet.cidr,
            name: subnet.name,
            family,
            used,
            total,
            conflicts,
            observed: subnet.observed,
            local,
        });
    }

    Ok(summaries)
}

/// 讀取全部網段（含 pools）；供匯出使用。
///
/// 兩筆查詢完成（網段＋全部 pools 後依 subnet_id 分組），避免逐網段 N+1。
pub async fn list_full(pool: &SqlitePool) -> sqlx::Result<Vec<Subnet>> {
    let rows =
        sqlx::query_as::<_, SubnetRow>(&format!("SELECT {COLUMNS} FROM subnets ORDER BY id ASC"))
            .fetch_all(pool)
            .await?;

    let pool_rows = sqlx::query_as::<_, PoolJoinRow>(
        "SELECT subnet_id, id, start_ip, end_ip FROM subnet_pools ORDER BY id ASC",
    )
    .fetch_all(pool)
    .await?;

    let mut pools_by_subnet: HashMap<i64, Vec<Pool>> = HashMap::new();
    for row in pool_rows {
        pools_by_subnet
            .entry(row.subnet_id)
            .or_default()
            .push(Pool {
                id: row.id,
                start_ip: row.start_ip,
                end_ip: row.end_ip,
            });
    }

    Ok(rows
        .into_iter()
        .map(|row| {
            let pools = pools_by_subnet.remove(&row.id).unwrap_or_default();
            row.into_subnet(pools)
        })
        .collect())
}

/// 全部網段轉 CSV（UTF-8 BOM＋標題列；格式見 ADR-0009）。
///
/// 列排序：v4 先、v6 後；同族依 CIDR 網路位址數值升冪。v6 的
/// Kea subnet-id 與位址池一律留空；選填欄位缺值為空字串。
pub fn export_csv(subnets: &[Subnet]) -> Result<Vec<u8>, ApiError> {
    let mut ordered: Vec<(&Subnet, IpNet)> = Vec::with_capacity(subnets.len());
    for subnet in subnets {
        let network = subnet
            .cidr
            .parse::<IpNet>()
            .map_err(|error| ApiError::internal("網段 CIDR 格式錯誤", error))?;
        ordered.push((subnet, network));
    }
    ordered.sort_by(|(_, a), (_, b)| compare_networks(a, b));

    let mut writer = csv::Writer::from_writer(Vec::new());
    writer
        .write_record(["名稱", "CIDR", "Gateway", "Kea subnet-id", "位址池", "備註"])
        .map_err(write_error)?;

    for (subnet, network) in ordered {
        let is_v4 = matches!(network, IpNet::V4(_));

        let kea_subnet_id = if is_v4 {
            subnet
                .kea_subnet_id
                .map(|id| id.to_string())
                .unwrap_or_default()
        } else {
            String::new()
        };
        let pools = if is_v4 {
            subnet
                .pools
                .iter()
                .map(|pool| format!("{}-{}", pool.start_ip, pool.end_ip))
                .collect::<Vec<_>>()
                .join("|")
        } else {
            String::new()
        };

        writer
            .write_record([
                subnet.name.as_deref().unwrap_or(""),
                subnet.cidr.as_str(),
                subnet.gateway.as_deref().unwrap_or(""),
                kea_subnet_id.as_str(),
                pools.as_str(),
                subnet.note.as_deref().unwrap_or(""),
            ])
            .map_err(write_error)?;
    }

    let body = writer
        .into_inner()
        .map_err(|error| ApiError::internal("產生網段匯出 CSV 失敗", error))?;

    let mut bytes = Vec::with_capacity(body.len() + 3);
    bytes.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
    bytes.extend_from_slice(&body);
    Ok(bytes)
}

/// CSV 寫入錯誤一律視為內部錯誤（寫入目標為記憶體緩衝區）。
fn write_error(error: csv::Error) -> ApiError {
    ApiError::internal("產生網段匯出 CSV 失敗", error)
}

/// 匯出列排序：v4 先、v6 後；同族依網路位址數值升冪。
fn compare_networks(a: &IpNet, b: &IpNet) -> Ordering {
    match (a, b) {
        (IpNet::V4(a), IpNet::V4(b)) => a.network().cmp(&b.network()),
        (IpNet::V6(a), IpNet::V6(b)) => a.network().cmp(&b.network()),
        (IpNet::V4(_), IpNet::V6(_)) => Ordering::Less,
        (IpNet::V6(_), IpNet::V4(_)) => Ordering::Greater,
    }
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

/// 於既有連線（可為交易）內新增網段與其 pools，回傳新列 id；不讀回完整資料。
///
/// 供匯入在同一交易內建立網段（見票 02）；一般建立路徑走 [`create`]。
pub(crate) async fn insert_subnet(
    connection: &mut SqliteConnection,
    valid: ValidSubnet,
) -> sqlx::Result<i64> {
    let result = sqlx::query(
        "INSERT INTO subnets (cidr, name, note, gateway, kea_subnet_id, observed)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(valid.cidr.to_string())
    .bind(valid.name)
    .bind(valid.note)
    .bind(valid.gateway.map(|address| address.to_string()))
    .bind(valid.kea_subnet_id)
    .bind(valid.observed)
    .execute(&mut *connection)
    .await?;

    let id = result.last_insert_rowid();
    insert_pools(connection, id, &valid.pools).await?;
    Ok(id)
}

/// 新增網段與其 pools（同一交易）。
pub async fn create(pool: &SqlitePool, valid: ValidSubnet) -> sqlx::Result<Subnet> {
    let mut transaction = pool.begin().await?;
    let id = insert_subnet(&mut transaction, valid).await?;
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
                 observed = ?,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
           WHERE id = ?",
    )
    .bind(valid.cidr.to_string())
    .bind(valid.name)
    .bind(valid.note)
    .bind(valid.gateway.map(|address| address.to_string()))
    .bind(valid.kea_subnet_id)
    .bind(valid.observed)
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
    connection: &mut SqliteConnection,
    subnet_id: i64,
    pools: &[ValidPool],
) -> sqlx::Result<()> {
    for pool in pools {
        sqlx::query("INSERT INTO subnet_pools (subnet_id, start_ip, end_ip) VALUES (?, ?, ?)")
            .bind(subnet_id)
            .bind(pool.start.to_string())
            .bind(pool.end.to_string())
            .execute(&mut *connection)
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
            observed: self.observed != 0,
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

    /// 匯出測試用：以最小內容組出網段（id 與時間不影響匯出內容）。
    fn export_subnet(
        cidr: &str,
        name: Option<&str>,
        gateway: Option<&str>,
        kea_subnet_id: Option<i64>,
        pools: &[(&str, &str)],
        note: Option<&str>,
    ) -> Subnet {
        Subnet {
            id: 0,
            cidr: cidr.to_string(),
            name: name.map(str::to_string),
            note: note.map(str::to_string),
            gateway: gateway.map(str::to_string),
            kea_subnet_id,
            observed: false,
            pools: pools
                .iter()
                .enumerate()
                .map(|(index, (start, end))| Pool {
                    id: index as i64 + 1,
                    start_ip: start.to_string(),
                    end_ip: end.to_string(),
                })
                .collect(),
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    #[test]
    fn export_csv_orders_v4_then_v6_and_formats_columns() {
        let subnets = vec![
            export_subnet(
                "fd00::/64",
                Some("v6 二"),
                Some("fd00::1"),
                None,
                &[],
                Some("v6 備註"),
            ),
            export_subnet("192.168.0.0/24", None, None, None, &[], None),
            export_subnet(
                "2001:db8::/64",
                Some("v6 一"),
                Some("2001:db8::1"),
                None,
                &[],
                None,
            ),
            export_subnet(
                "10.0.0.0/24",
                Some("辦公區"),
                Some("10.0.0.1"),
                Some(10),
                &[("10.0.0.100", "10.0.0.150"), ("10.0.0.200", "10.0.0.220")],
                Some("三樓,近電梯"),
            ),
        ];

        let bytes = export_csv(&subnets).expect("匯出成功");
        assert_eq!(&bytes[..3], &[0xEF, 0xBB, 0xBF], "須有 UTF-8 BOM");

        let mut reader = csv::Reader::from_reader(&bytes[3..]);
        let headers: Vec<String> = reader
            .headers()
            .expect("標題列")
            .iter()
            .map(str::to_string)
            .collect();
        assert_eq!(
            headers,
            ["名稱", "CIDR", "Gateway", "Kea subnet-id", "位址池", "備註"]
        );

        let rows: Vec<csv::StringRecord> = reader
            .into_records()
            .map(|record| record.expect("資料列"))
            .collect();
        assert_eq!(rows.len(), 4);

        assert_eq!(rows[0].get(1), Some("10.0.0.0/24"), "v4 依網路位址升冪");
        assert_eq!(rows[0].get(0), Some("辦公區"));
        assert_eq!(rows[0].get(2), Some("10.0.0.1"));
        assert_eq!(rows[0].get(3), Some("10"));
        assert_eq!(
            rows[0].get(4),
            Some("10.0.0.100-10.0.0.150|10.0.0.200-10.0.0.220"),
            "多段 pool 以 | 分隔、每段 起點-終點"
        );
        assert_eq!(rows[0].get(5), Some("三樓,近電梯"), "含逗號欄位經引號往返");

        assert_eq!(rows[1].get(1), Some("192.168.0.0/24"));
        for column in [0, 2, 3, 4, 5] {
            assert_eq!(rows[1].get(column), Some(""), "選填欄位缺值為空字串");
        }

        assert_eq!(
            rows[2].get(1),
            Some("2001:db8::/64"),
            "v6 排在 v4 後；同族依網路位址數值升冪"
        );
        assert_eq!(rows[2].get(0), Some("v6 一"));
        assert_eq!(rows[2].get(2), Some("2001:db8::1"));
        assert_eq!(rows[2].get(3), Some(""));
        assert_eq!(rows[2].get(4), Some(""));
        assert_eq!(rows[3].get(1), Some("fd00::/64"));
        assert_eq!(rows[3].get(0), Some("v6 二"));
        assert_eq!(rows[3].get(2), Some("fd00::1"));
        assert_eq!(rows[3].get(3), Some(""));
        assert_eq!(rows[3].get(4), Some(""));
        assert_eq!(rows[3].get(5), Some("v6 備註"));
    }

    #[test]
    fn export_csv_leaves_v6_kea_and_pools_empty() {
        // 防禦性：v6 資料即使異常帶值，輸出仍依 ADR-0009 留空。
        let subnet = export_subnet(
            "fd00::/64",
            None,
            None,
            Some(9),
            &[("fd00::10", "fd00::20")],
            None,
        );
        let bytes = export_csv(&[subnet]).expect("匯出成功");

        let mut reader = csv::Reader::from_reader(&bytes[3..]);
        let row = reader.records().next().expect("資料列").expect("資料列");
        assert_eq!(row.get(3), Some(""), "v6 的 Kea subnet-id 留空");
        assert_eq!(row.get(4), Some(""), "v6 的位址池留空");
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

    /// 測試用已驗證網段（含一段 pool）。
    fn valid_subnet() -> ValidSubnet {
        validate_fields(
            Some("10.0.0.0/24".to_string()),
            Some("測試網段".to_string()),
            None,
            None,
            None,
            vec![PoolInput {
                start_ip: Some("10.0.0.10".to_string()),
                end_ip: Some("10.0.0.20".to_string()),
            }],
        )
        .expect("有效網段")
    }

    #[tokio::test]
    async fn insert_subnet_is_visible_in_same_transaction() {
        let pool = test_pool().await;
        let mut transaction = pool.begin().await.expect("建立交易");

        let id = insert_subnet(&mut transaction, valid_subnet())
            .await
            .expect("新增網段");
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM subnets WHERE id = ?")
            .bind(id)
            .fetch_one(&mut *transaction)
            .await
            .expect("同交易讀取網段");
        let pools: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM subnet_pools WHERE subnet_id = ?")
                .bind(id)
                .fetch_one(&mut *transaction)
                .await
                .expect("同交易讀取 pools");

        assert_eq!(count, 1, "原語建立後同交易可見");
        assert_eq!(pools, 1, "pools 於同一交易寫入");
    }

    #[tokio::test]
    async fn insert_subnet_rollback_leaves_nothing() {
        let pool = test_pool().await;
        let mut transaction = pool.begin().await.expect("建立交易");

        insert_subnet(&mut transaction, valid_subnet())
            .await
            .expect("新增網段");
        transaction.rollback().await.expect("回滾交易");

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM subnets")
            .fetch_one(&pool)
            .await
            .expect("回滾後讀取網段");
        let pools: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM subnet_pools")
            .fetch_one(&pool)
            .await
            .expect("回滾後讀取 pools");

        assert_eq!(count, 0, "回滾後不留下網段");
        assert_eq!(pools, 0, "回滾後不留下 pools");
    }
}
