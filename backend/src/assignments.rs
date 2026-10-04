//! 指派（Assignment）領域模組：指派／改用途、v6 登錄、取消與指派對象查詢。
//!
//! 詞彙依 `CONTEXT.md`；規則見 `.scratch/asset-ip-management/spec.md` §2.4、§3.1、
//! §4.3、§5，決策見 ADR-0005（指派以 Interface 為對象）與 ADR-0006（結構錯誤阻擋）。
//! v6 採登錄制：新增即指派、用途固定 static；語意衝突偵測見 [`crate::conflicts`]
//! （票 07）——更新既有指派時，出界／落池不再重驗、改以標記呈現。

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use ipnet::IpNet;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{FromRow, SqlitePool};

use crate::api::ApiError;
use crate::assets::{self, optional_text};
use crate::interfaces;
use crate::ips::HostRange;
use crate::subnets::{self, Subnet};

/// `ip_assignments` 資料表完整欄位清單。
const COLUMNS: &str =
    "id, subnet_id, address, interface_id, purpose, hostname, created_at, updated_at";

/// `ip_assignments` 資料表列。
#[derive(Debug, FromRow)]
struct AssignmentRow {
    id: i64,
    subnet_id: i64,
    address: String,
    interface_id: i64,
    purpose: String,
    hostname: Option<String>,
    created_at: String,
    updated_at: String,
}

/// 目前指派對象（資產＋介面）；供資產端移轉提示（見票 10）。
#[derive(Debug, FromRow)]
struct AssignmentTargetRow {
    asset_id: i64,
    asset_description: String,
    asset_location: String,
    interface_id: i64,
    interface_name: Option<String>,
    mac: Option<String>,
}

/// API 回傳的指派（僅記目前狀態，無歷程）。
#[derive(Debug, Serialize)]
pub struct Assignment {
    pub id: i64,
    pub subnet_id: i64,
    pub address: String,
    pub interface_id: i64,
    /// `static`（手動設定）或 `reservation`（DHCPv4 保留）。
    pub purpose: String,
    pub hostname: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// 指派／改用途的輸入；缺漏欄位由 `validate` 回報。
#[derive(Debug, Default, Deserialize)]
pub struct AssignmentInput {
    pub interface_id: Option<i64>,
    pub purpose: Option<String>,
    pub hostname: Option<String>,
}

/// 已驗證的指派內容。
#[derive(Debug)]
pub struct ValidAssignment {
    interface_id: i64,
    purpose: &'static str,
    hostname: Option<String>,
}

impl AssignmentInput {
    /// 驗證輸入；結構錯誤阻擋儲存（見 ADR-0006）。
    pub fn validate(self) -> Result<ValidAssignment, ApiError> {
        let interface_id = self
            .interface_id
            .ok_or_else(|| ApiError::validation("介面為必填").field("interface_id"))?;

        let purpose = match self.purpose.as_deref().map(str::trim) {
            Some("static") => "static",
            Some("reservation") => "reservation",
            Some(_) => {
                return Err(ApiError::validation(
                    "用途須為 static（手動設定）或 reservation（保留）",
                )
                .field("purpose"));
            }
            None => return Err(ApiError::validation("用途為必填").field("purpose")),
        };

        let hostname = optional_text(self.hostname);
        if purpose == "static" && hostname.is_some() {
            return Err(ApiError::validation("僅保留用途可填 hostname").field("hostname"));
        }

        Ok(ValidAssignment {
            interface_id,
            purpose,
            hostname,
        })
    }
}

/// 資產端指派的輸入（見票 10）；缺漏欄位由 `validate` 回報。
///
/// `address` 為完整 IP（v4／v6），由後端反推所屬網段；`transfer` 為
/// 位址已指派給其他介面時，是否確認移轉（預設 `false`）。
#[derive(Debug, Default, Deserialize)]
pub struct AssetAssignmentInput {
    pub address: Option<String>,
    pub interface_id: Option<i64>,
    pub purpose: Option<String>,
    pub hostname: Option<String>,
    #[serde(default)]
    pub transfer: bool,
}

impl AssetAssignmentInput {
    /// 驗證輸入；回傳（位址、是否移轉、已驗證的指派內容）。
    ///
    /// 位址須為合法 IP；介面、用途與 hostname 的規則沿用 [`AssignmentInput`]。
    pub fn validate(self) -> Result<(IpAddr, bool, ValidAssignment), ApiError> {
        let text = optional_text(self.address)
            .ok_or_else(|| ApiError::validation("位址為必填").field("address"))?;
        let address: IpAddr = text.parse().map_err(|_| {
            ApiError::validation(format!("位址格式錯誤：{text}（須為合法 IP 位址）"))
                .field("address")
        })?;

        let valid = AssignmentInput {
            interface_id: self.interface_id,
            purpose: self.purpose,
            hostname: self.hostname,
        }
        .validate()?;

        Ok((address, self.transfer, valid))
    }
}

/// v6 登錄位址的輸入（新增即指派）；用途固定 static、無 hostname。
#[derive(Debug, Default, Deserialize)]
pub struct RegisterInput {
    pub address: Option<String>,
    pub interface_id: Option<i64>,
}

impl RegisterInput {
    /// 驗證輸入；位址須為合法 IPv6（是否落在網段內由 [`register`] 檢查）。
    pub fn validate(self) -> Result<(Ipv6Addr, i64), ApiError> {
        let text = optional_text(self.address)
            .ok_or_else(|| ApiError::validation("位址為必填").field("address"))?;
        let address: Ipv6Addr = text.parse().map_err(|_| {
            ApiError::validation(format!("位址格式錯誤：{text}（須為合法 IPv6 位址）"))
                .field("address")
        })?;

        let interface_id = self
            .interface_id
            .ok_or_else(|| ApiError::validation("介面為必填").field("interface_id"))?;

        Ok((address, interface_id))
    }
}

/// IP 清單中的指派對象（見 spec §4.3）。
#[derive(Debug, Clone, Serialize)]
pub struct IpAssignment {
    pub asset_id: i64,
    pub asset_description: String,
    pub asset_location: String,
    pub interface_id: i64,
    pub interface_name: Option<String>,
    pub mac: Option<String>,
    pub hostname: Option<String>,
}

/// 某網段的指派列；供 IP 清單合併（`address` 為正規化後的文字）。
#[derive(Debug, FromRow)]
pub struct ListedAssignment {
    pub address: String,
    pub purpose: String,
    pub hostname: Option<String>,
    pub interface_id: i64,
    pub interface_name: Option<String>,
    pub mac: Option<String>,
    pub asset_id: i64,
    pub asset_description: String,
    pub asset_location: String,
}

impl ListedAssignment {
    /// 取出指派對象（資產＋介面），供 IP 列巢狀顯示。
    pub fn target(&self) -> IpAssignment {
        IpAssignment {
            asset_id: self.asset_id,
            asset_description: self.asset_description.clone(),
            asset_location: self.asset_location.clone(),
            interface_id: self.interface_id,
            interface_name: self.interface_name.clone(),
            mac: self.mac.clone(),
            hostname: self.hostname.clone(),
        }
    }
}

/// 資產詳情中的已指派 IP（唯讀顯示；見 spec §4.1）。
#[derive(Debug, FromRow, Serialize)]
pub struct AssetAssignment {
    pub id: i64,
    pub subnet_id: i64,
    pub subnet_cidr: String,
    pub subnet_name: Option<String>,
    pub address: String,
    pub purpose: String,
    pub hostname: Option<String>,
    pub interface_id: i64,
    pub interface_name: Option<String>,
    pub mac: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// 指派位址（或改用途）；結構錯誤阻擋儲存（見 spec §3.1）。
///
/// v4：新指派位址須為 host（扣除 network/broadcast）且不在 pool 內；
/// v6：登錄制，新指派位址須落在 CIDR 內（含 network 位址；無 host 扣除概念），
/// 用途固定 static。同一網段同一位址已指派給其他介面時阻擋，須先取消再
/// 重新指派（換介面即取消＋重新指派，見 CONTEXT.md／ADR-0005）。
///
/// 更新既有指派（同介面同位址）時不重驗出界／落池：網段編輯可能使既有
/// 位址出界或落池，該情形以語意衝突標記呈現、不阻擋儲存（見 ADR-0006、票 07）。
pub async fn assign(
    pool: &SqlitePool,
    subnet: &Subnet,
    address: IpAddr,
    valid: ValidAssignment,
) -> Result<Assignment, ApiError> {
    let address_text = address.to_string();

    let existing_address = fetch_by_address(pool, subnet.id, &address_text)
        .await
        .map_err(|error| ApiError::internal("讀取指派失敗", error))?;

    if let Some(existing) = &existing_address {
        if existing.interface_id != valid.interface_id {
            return Err(ApiError::validation("位址已被指派給其他介面，請先取消指派")
                .field("address")
                .detail("interface_id", json!(existing.interface_id)));
        }
    }

    // 同介面同位址＝更新：出界／落池為語意衝突，交由標記呈現（見票 07）。
    validate_address(subnet, address, valid.purpose, existing_address.is_some())?;

    let interface = interfaces::get(pool, valid.interface_id)
        .await
        .map_err(|error| ApiError::internal("讀取介面失敗", error))?
        .ok_or_else(|| ApiError::validation("找不到介面").field("interface_id"))?;

    if valid.purpose == "reservation" && interface.mac.is_none() {
        return Err(
            ApiError::validation("保留需介面有 MAC；無 MAC 的介面僅能手動設定")
                .field("purpose")
                .detail("interface_id", json!(interface.id)),
        );
    }

    if let Some(existing) = fetch_by_interface(pool, subnet.id, valid.interface_id)
        .await
        .map_err(|error| ApiError::internal("讀取指派失敗", error))?
    {
        if existing.address != address_text {
            return Err(ApiError::validation(format!(
                "此介面在此網段已有位址 {}（同一介面同一網段至多一個位址）",
                existing.address
            ))
            .field("interface_id")
            .detail("existing_address", json!(existing.address)));
        }
    }

    write(pool, subnet.id, &address_text, existing_address, valid).await
}

/// 資產端指派（見票 10）：由位址反推所屬網段，指派給該資產的指定介面。
///
/// 位址已指派給其他介面時：
/// - `transfer == false`：回 400 `address_assigned_elsewhere`，附目前指派對象
///   （供前端提示確認；見 ADR-0007）。
/// - `transfer == true`：同一交易內原子移轉（刪除舊指派＋建立新指派），
///   不留歷程；不重驗出界／落池，沿用既有位址的處理原則（見 ADR-0006）。
///
/// 結構規則不因移轉而放寬：保留需介面有 MAC、同一介面同一網段至多一位址、
/// v6 用途固定 static；位址未指派時 v4 須為 host 且非池內、v6 須落在 CIDR 內。
/// 位址已指派給同一介面＝更新用途／hostname（`transferred` 為 `false`）。
///
/// 回傳（指派、是否發生移轉）。
pub async fn assign_for_asset(
    pool: &SqlitePool,
    asset_id: i64,
    address: IpAddr,
    valid: ValidAssignment,
    transfer: bool,
) -> Result<(Assignment, bool), ApiError> {
    let address_text = address.to_string();

    // 資產存在；介面存在且屬於該資產（見票 10）。
    if assets::get(pool, asset_id)
        .await
        .map_err(|error| ApiError::internal("讀取資產失敗", error))?
        .is_none()
    {
        return Err(ApiError::not_found("找不到資產"));
    }

    let interface = interfaces::get(pool, valid.interface_id)
        .await
        .map_err(|error| ApiError::internal("讀取介面失敗", error))?
        .ok_or_else(|| ApiError::validation("找不到介面").field("interface_id"))?;

    if interface.asset_id != asset_id {
        return Err(ApiError::validation("介面不屬於此資產")
            .field("interface_id")
            .detail("asset_id", json!(interface.asset_id)));
    }

    // 由位址找所屬網段；網段不重疊，至多一個（見 spec §2.3、§3.1）。
    let subnet = subnets::find_by_address(pool, address)
        .await?
        .ok_or_else(|| {
            ApiError::validation(format!("位址 {address} 不在任何網段內"))
                .field("address")
                .detail("reason", json!("no_subnet"))
        })?;

    // 結構規則（不因移轉而放寬）：保留需 MAC；v6 用途固定 static。
    if valid.purpose == "reservation" && interface.mac.is_none() {
        return Err(
            ApiError::validation("保留需介面有 MAC；無 MAC 的介面僅能手動設定")
                .field("purpose")
                .detail("interface_id", json!(interface.id)),
        );
    }

    if address.is_ipv6() && valid.purpose != "static" {
        return Err(ApiError::validation("IPv6 位址用途固定為手動設定（static）").field("purpose"));
    }

    // 同一介面在同一網段至多一個位址：目標介面已有其他位址即阻擋。
    if let Some(existing) = fetch_by_interface(pool, subnet.id, valid.interface_id)
        .await
        .map_err(|error| ApiError::internal("讀取指派失敗", error))?
    {
        if existing.address != address_text {
            return Err(ApiError::validation(format!(
                "此介面在此網段已有位址 {}（同一介面同一網段至多一個位址）",
                existing.address
            ))
            .field("interface_id")
            .detail("existing_address", json!(existing.address)));
        }
    }

    let existing_address = fetch_by_address(pool, subnet.id, &address_text)
        .await
        .map_err(|error| ApiError::internal("讀取指派失敗", error))?;

    let mut transferred = false;
    if let Some(existing) = &existing_address {
        if existing.interface_id != valid.interface_id {
            if !transfer {
                return Err(assigned_elsewhere_error(pool, &subnet, existing).await?);
            }
            transferred = true;
        }
    }

    // 新指派驗證 host／pool；既有位址（更新或移轉）不重驗出界／落池。
    validate_address(&subnet, address, valid.purpose, existing_address.is_some())?;

    if transferred {
        let old = existing_address
            .as_ref()
            .ok_or_else(|| ApiError::internal("移轉指派失敗", "找不到舊指派"))?;
        let assignment = transfer_atomically(pool, subnet.id, &address_text, old.id, valid).await?;
        return Ok((assignment, true));
    }

    let assignment = write(pool, subnet.id, &address_text, existing_address, valid).await?;
    Ok((assignment, false))
}

/// 「位址已指派給其他介面」的 400 錯誤；`details` 附目前指派對象供前端提示
/// （見票 10、ADR-0007）。
async fn assigned_elsewhere_error(
    pool: &SqlitePool,
    subnet: &Subnet,
    existing: &AssignmentRow,
) -> Result<ApiError, ApiError> {
    let target = fetch_target(pool, existing.interface_id)
        .await
        .map_err(|error| ApiError::internal("讀取指派對象失敗", error))?
        .ok_or_else(|| ApiError::internal("讀取指派對象失敗", "介面不存在"))?;

    Ok(ApiError::validation(format!(
        "位址 {} 已指派給其他介面，請確認是否移轉",
        existing.address
    ))
    .field("address")
    .detail("reason", json!("address_assigned_elsewhere"))
    .detail("subnet_id", json!(subnet.id))
    .detail("subnet_cidr", json!(subnet.cidr))
    .detail("asset_id", json!(target.asset_id))
    .detail("asset_description", json!(target.asset_description))
    .detail("asset_location", json!(target.asset_location))
    .detail("interface_id", json!(target.interface_id))
    .detail("interface_name", json!(target.interface_name))
    .detail("mac", json!(target.mac)))
}

/// 原子移轉：同一交易內刪除舊指派、建立新指派；不留歷程（見 ADR-0007）。
async fn transfer_atomically(
    pool: &SqlitePool,
    subnet_id: i64,
    address: &str,
    old_id: i64,
    valid: ValidAssignment,
) -> Result<Assignment, ApiError> {
    let mut transaction = pool
        .begin()
        .await
        .map_err(|error| ApiError::internal("建立移轉交易失敗", error))?;

    sqlx::query("DELETE FROM ip_assignments WHERE id = ?")
        .bind(old_id)
        .execute(&mut *transaction)
        .await
        .map_err(|error| ApiError::internal("刪除舊指派失敗", error))?;

    let result = sqlx::query(
        "INSERT INTO ip_assignments (subnet_id, address, interface_id, purpose, hostname)
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(subnet_id)
    .bind(address)
    .bind(valid.interface_id)
    .bind(valid.purpose)
    .bind(valid.hostname)
    .execute(&mut *transaction)
    .await
    .map_err(map_write_error)?;

    let id = result.last_insert_rowid();
    transaction
        .commit()
        .await
        .map_err(|error| ApiError::internal("提交移轉交易失敗", error))?;

    fetch_row(pool, id)
        .await
        .map_err(|error| ApiError::internal("讀取指派失敗", error))?
        .map(AssignmentRow::into_assignment)
        .ok_or_else(|| ApiError::internal("讀取指派失敗", "列不存在"))
}

/// v6 登錄位址（新增即指派）：建立即為手動設定（static）。
///
/// 僅適用 IPv6 網段；v4 網段呼叫回 400（v4 走 [`assign`]／既有指派端點）。
/// 同網段同一位址已登錄即回 400（含同一介面；不採冪等更新）。
pub async fn register(
    pool: &SqlitePool,
    subnet: &Subnet,
    address: Ipv6Addr,
    interface_id: i64,
) -> Result<Assignment, ApiError> {
    let network = parse_network(&subnet.cidr)?;
    if !network.addr().is_ipv6() {
        return Err(ApiError::validation("此端點僅適用於 IPv6 網段（v6 登錄制）").field("cidr"));
    }

    if let Some(existing) = fetch_by_address(pool, subnet.id, &address.to_string())
        .await
        .map_err(|error| ApiError::internal("讀取指派失敗", error))?
    {
        return Err(ApiError::validation(format!(
            "位址 {address} 已登錄（同一網段同一位址僅能一筆）"
        ))
        .field("address")
        .detail("interface_id", json!(existing.interface_id)));
    }

    let valid = ValidAssignment {
        interface_id,
        purpose: "static",
        hostname: None,
    };
    assign(pool, subnet, IpAddr::V6(address), valid).await
}

/// 取消指派；回傳是否確實刪除（不存在回傳 `false`）。v6 即刪除登錄。
pub async fn cancel(pool: &SqlitePool, subnet_id: i64, address: IpAddr) -> sqlx::Result<bool> {
    let result = sqlx::query("DELETE FROM ip_assignments WHERE subnet_id = ? AND address = ?")
        .bind(subnet_id)
        .bind(address.to_string())
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

/// 位址結構驗證：地址族須與網段相符、落在 CIDR 內（v4 另扣 network/broadcast
/// 與 pool）；v6 用途固定 static（見 spec §2.4、§7）。
///
/// `existing`＝同介面同位址的更新：出界／落池為語意衝突（網段編輯造成），
/// 不重驗、由標記呈現（見 ADR-0006、票 07）；地址族與 v6 用途仍為結構規則。
///
/// 供匯入以「新指派」情境（`existing = false`）重用（見票 01）。
pub(crate) fn validate_address(
    subnet: &Subnet,
    address: IpAddr,
    purpose: &str,
    existing: bool,
) -> Result<(), ApiError> {
    let network = parse_network(&subnet.cidr)?;

    match (network, address) {
        (IpNet::V4(network), IpAddr::V4(address)) => {
            if !existing {
                let range = HostRange::of(&network);
                if !range.contains(address) {
                    return Err(ApiError::validation(format!(
                        "位址 {address} 不在網段 {} 的可用範圍內（network/broadcast 不可指派）",
                        subnet.cidr
                    ))
                    .field("address")
                    .detail("cidr", json!(subnet.cidr)));
                }

                if is_in_pool(subnet, address)? {
                    return Err(ApiError::validation(format!(
                        "位址 {address} 落在 DHCP 位址池內，不可指派"
                    ))
                    .field("address"));
                }
            }
        }
        (IpNet::V6(network), IpAddr::V6(address)) => {
            if !existing && !network.contains(&address) {
                return Err(ApiError::validation(format!(
                    "位址 {address} 不在網段 {} 內",
                    subnet.cidr
                ))
                .field("address")
                .detail("cidr", json!(subnet.cidr)));
            }

            if purpose != "static" {
                return Err(
                    ApiError::validation("IPv6 位址用途固定為手動設定（static）").field("purpose"),
                );
            }
        }
        _ => {
            return Err(ApiError::validation(format!(
                "位址 {address} 與網段 {} 的地址族不符",
                subnet.cidr
            ))
            .field("address")
            .detail("cidr", json!(subnet.cidr)));
        }
    }

    Ok(())
}

/// 某網段的指派筆數（static＋reservation）；供網段刪除防護（見票 08）。
pub async fn count_for_subnet(pool: &SqlitePool, subnet_id: i64) -> sqlx::Result<i64> {
    sqlx::query_scalar("SELECT COUNT(*) FROM ip_assignments WHERE subnet_id = ?")
        .bind(subnet_id)
        .fetch_one(pool)
        .await
}

/// 讀取某網段的全部指派（含資產與介面資訊），供 IP 清單合併。
pub async fn list_for_subnet(
    pool: &SqlitePool,
    subnet_id: i64,
) -> sqlx::Result<Vec<ListedAssignment>> {
    sqlx::query_as::<_, ListedAssignment>(
        "SELECT a.address, a.purpose, a.hostname, a.interface_id,
                i.name AS interface_name, i.mac,
                s.id AS asset_id, s.description AS asset_description,
                s.location AS asset_location
           FROM ip_assignments a
           JOIN interfaces i ON i.id = a.interface_id
           JOIN assets s ON s.id = i.asset_id
          WHERE a.subnet_id = ?
          ORDER BY a.id ASC",
    )
    .bind(subnet_id)
    .fetch_all(pool)
    .await
}

/// 讀取某資產（經由介面）的已指派 IP，含網段資訊；供資產詳情唯讀顯示。
pub async fn list_for_asset(
    pool: &SqlitePool,
    asset_id: i64,
) -> sqlx::Result<Vec<AssetAssignment>> {
    sqlx::query_as::<_, AssetAssignment>(
        "SELECT a.id, a.subnet_id, n.cidr AS subnet_cidr, n.name AS subnet_name,
                a.address, a.purpose, a.hostname, a.interface_id,
                i.name AS interface_name, i.mac, a.created_at, a.updated_at
           FROM ip_assignments a
           JOIN interfaces i ON i.id = a.interface_id
           JOIN subnets n ON n.id = a.subnet_id
          WHERE i.asset_id = ?
          ORDER BY a.subnet_id ASC, a.id ASC",
    )
    .bind(asset_id)
    .fetch_all(pool)
    .await
}

/// 新增或更新指派列；呼叫端已完成結構驗證。
async fn write(
    pool: &SqlitePool,
    subnet_id: i64,
    address: &str,
    existing: Option<AssignmentRow>,
    valid: ValidAssignment,
) -> Result<Assignment, ApiError> {
    let id = match existing {
        Some(row) => {
            sqlx::query(
                "UPDATE ip_assignments
                     SET purpose = ?, hostname = ?,
                         updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
                   WHERE id = ?",
            )
            .bind(valid.purpose)
            .bind(valid.hostname)
            .bind(row.id)
            .execute(pool)
            .await
            .map_err(map_write_error)?;
            row.id
        }
        None => {
            let result = sqlx::query(
                "INSERT INTO ip_assignments (subnet_id, address, interface_id, purpose, hostname)
                 VALUES (?, ?, ?, ?, ?)",
            )
            .bind(subnet_id)
            .bind(address)
            .bind(valid.interface_id)
            .bind(valid.purpose)
            .bind(valid.hostname)
            .execute(pool)
            .await
            .map_err(map_write_error)?;
            result.last_insert_rowid()
        }
    };

    fetch_row(pool, id)
        .await
        .map_err(|error| ApiError::internal("讀取指派失敗", error))?
        .map(AssignmentRow::into_assignment)
        .ok_or_else(|| ApiError::internal("讀取指派失敗", "列不存在"))
}

/// 資料庫唯一性限制為結構規則的雙保險（見 spec §3.1）；衝突時回 400。
fn map_write_error(error: sqlx::Error) -> ApiError {
    if let sqlx::Error::Database(database_error) = &error {
        if database_error.is_unique_violation() {
            return ApiError::validation(
                "指派違反唯一性限制（同一網段同一位址或同一介面同一網段僅能一筆）",
            );
        }
    }
    ApiError::internal("寫入指派失敗", error)
}

/// 解析網段 CIDR（v4／v6）；資料異常回傳內部錯誤。
fn parse_network(cidr: &str) -> Result<IpNet, ApiError> {
    cidr.parse()
        .map_err(|error| ApiError::internal("網段 CIDR 格式錯誤", error))
}

/// 位址是否落在網段任一 pool 內（端點皆含）；池內位址不可指派（見 spec §7）。
fn is_in_pool(subnet: &Subnet, address: Ipv4Addr) -> Result<bool, ApiError> {
    for pool in &subnet.pools {
        let start: Ipv4Addr = pool
            .start_ip
            .parse()
            .map_err(|error| ApiError::internal("pool 位址格式錯誤", error))?;
        let end: Ipv4Addr = pool
            .end_ip
            .parse()
            .map_err(|error| ApiError::internal("pool 位址格式錯誤", error))?;
        if start <= address && address <= end {
            return Ok(true);
        }
    }
    Ok(false)
}

/// 讀取一列；不存在回傳 `None`。
async fn fetch_row(pool: &SqlitePool, id: i64) -> sqlx::Result<Option<AssignmentRow>> {
    sqlx::query_as::<_, AssignmentRow>(&format!(
        "SELECT {COLUMNS} FROM ip_assignments WHERE id = ?"
    ))
    .bind(id)
    .fetch_optional(pool)
    .await
}

/// 依網段＋位址讀取一列。
async fn fetch_by_address(
    pool: &SqlitePool,
    subnet_id: i64,
    address: &str,
) -> sqlx::Result<Option<AssignmentRow>> {
    sqlx::query_as::<_, AssignmentRow>(&format!(
        "SELECT {COLUMNS} FROM ip_assignments WHERE subnet_id = ? AND address = ?"
    ))
    .bind(subnet_id)
    .bind(address)
    .fetch_optional(pool)
    .await
}

/// 依網段＋介面讀取一列。
async fn fetch_by_interface(
    pool: &SqlitePool,
    subnet_id: i64,
    interface_id: i64,
) -> sqlx::Result<Option<AssignmentRow>> {
    sqlx::query_as::<_, AssignmentRow>(&format!(
        "SELECT {COLUMNS} FROM ip_assignments WHERE subnet_id = ? AND interface_id = ?"
    ))
    .bind(subnet_id)
    .bind(interface_id)
    .fetch_optional(pool)
    .await
}

/// 讀取指派對象（資產＋介面）；介面不存在回傳 `None`。
async fn fetch_target(
    pool: &SqlitePool,
    interface_id: i64,
) -> sqlx::Result<Option<AssignmentTargetRow>> {
    sqlx::query_as::<_, AssignmentTargetRow>(
        "SELECT s.id AS asset_id, s.description AS asset_description,
                s.location AS asset_location, i.id AS interface_id,
                i.name AS interface_name, i.mac
           FROM interfaces i
           JOIN assets s ON s.id = i.asset_id
          WHERE i.id = ?",
    )
    .bind(interface_id)
    .fetch_optional(pool)
    .await
}

impl AssignmentRow {
    fn into_assignment(self) -> Assignment {
        Assignment {
            id: self.id,
            subnet_id: self.subnet_id,
            address: self.address,
            interface_id: self.interface_id,
            purpose: self.purpose,
            hostname: self.hostname,
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_requires_interface_and_valid_purpose() {
        assert!(
            AssignmentInput {
                interface_id: None,
                purpose: Some("static".to_string()),
                hostname: None,
            }
            .validate()
            .is_err(),
            "缺介面應阻擋"
        );
        assert!(
            AssignmentInput {
                interface_id: Some(1),
                purpose: None,
                hostname: None,
            }
            .validate()
            .is_err(),
            "缺用途應阻擋"
        );
        assert!(
            AssignmentInput {
                interface_id: Some(1),
                purpose: Some("dhcp".to_string()),
                hostname: None,
            }
            .validate()
            .is_err(),
            "未知用途應阻擋"
        );
    }

    #[test]
    fn hostname_is_trimmed_and_only_for_reservation() {
        let valid = AssignmentInput {
            interface_id: Some(1),
            purpose: Some("reservation".to_string()),
            hostname: Some("  host-1  ".to_string()),
        }
        .validate()
        .expect("保留可填 hostname");
        assert_eq!(valid.hostname.as_deref(), Some("host-1"));

        let valid = AssignmentInput {
            interface_id: Some(1),
            purpose: Some("static".to_string()),
            hostname: Some("   ".to_string()),
        }
        .validate()
        .expect("全空白視為未填");
        assert_eq!(valid.hostname, None);

        assert!(
            AssignmentInput {
                interface_id: Some(1),
                purpose: Some("static".to_string()),
                hostname: Some("host-1".to_string()),
            }
            .validate()
            .is_err(),
            "手動設定不可填 hostname"
        );
    }

    #[test]
    fn asset_assignment_input_requires_address_and_parses_families() {
        let (address, transfer, valid) = AssetAssignmentInput {
            address: Some(" 10.0.0.5 ".to_string()),
            interface_id: Some(7),
            purpose: Some("static".to_string()),
            hostname: None,
            transfer: true,
        }
        .validate()
        .expect("合法輸入");
        assert_eq!(address.to_string(), "10.0.0.5");
        assert!(transfer);
        assert_eq!(valid.interface_id, 7);

        let (address, _, _) = AssetAssignmentInput {
            address: Some("fd00::5".to_string()),
            interface_id: Some(7),
            purpose: Some("static".to_string()),
            hostname: None,
            transfer: false,
        }
        .validate()
        .expect("v6 亦為合法位址");
        assert!(address.is_ipv6());

        assert!(
            AssetAssignmentInput {
                address: None,
                ..Default::default()
            }
            .validate()
            .is_err(),
            "缺位址應阻擋"
        );
        assert!(
            AssetAssignmentInput {
                address: Some("abc".to_string()),
                ..Default::default()
            }
            .validate()
            .is_err(),
            "非法位址應阻擋"
        );
        assert!(
            AssetAssignmentInput {
                address: Some("10.0.0.5".to_string()),
                ..Default::default()
            }
            .validate()
            .is_err(),
            "缺介面／用途應阻擋"
        );
    }

    #[test]
    fn asset_assignment_input_transfer_defaults_to_false() {
        let input: AssetAssignmentInput = serde_json::from_value(serde_json::json!({
            "address": "10.0.0.5",
            "interface_id": 1,
            "purpose": "static"
        }))
        .expect("反序列化成功");
        assert!(!input.transfer, "未提供 transfer 視為 false");
    }

    #[test]
    fn register_input_requires_ipv6_address_and_interface() {
        assert!(
            RegisterInput {
                address: None,
                interface_id: Some(1),
            }
            .validate()
            .is_err(),
            "缺位址應阻擋"
        );
        assert!(
            RegisterInput {
                address: Some("fd00::1".to_string()),
                interface_id: None,
            }
            .validate()
            .is_err(),
            "缺介面應阻擋"
        );
        for invalid in ["abc", "10.0.0.1", "fd00::gg", "fd00::1/64"] {
            assert!(
                RegisterInput {
                    address: Some(invalid.to_string()),
                    interface_id: Some(1),
                }
                .validate()
                .is_err(),
                "{invalid} 非合法 IPv6 應阻擋"
            );
        }

        let (address, interface_id) = RegisterInput {
            address: Some(" fd00:0:0:0:0:0:0:1 ".to_string()),
            interface_id: Some(7),
        }
        .validate()
        .expect("合法輸入");
        assert_eq!(address.to_string(), "fd00::1", "解析後為壓縮正規形式");
        assert_eq!(interface_id, 7);
    }
}
