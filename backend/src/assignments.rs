//! 指派（Assignment）領域模組：指派／改用途、v6 登錄、取消與指派對象查詢。
//!
//! 詞彙依 `GLOSSARY.md`；規則見 `.scratch/asset-ip-management/spec.md` §2.4、§3.1、
//! §4.3、§5，決策見 ADR-0005（指派以 Interface 為對象）與 ADR-0006（結構錯誤阻擋）。
//! v6 採登錄制：新增即指派、用途固定 static；語意衝突偵測見 [`crate::conflicts`]
//! （票 07）——更新既有指派時，出界／落池不再重驗、改以標記呈現。

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use ipnet::IpNet;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{FromRow, QueryBuilder, SqlitePool};

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

/// 目前指派對象（資產＋介面）；供資產端移轉提示（見票 10、票 16）。
#[derive(Debug, FromRow)]
struct AssignmentTargetRow {
    asset_id: i64,
    asset_property_no: Option<String>,
    asset_description: String,
    asset_brand: Option<String>,
    asset_model: Option<String>,
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
    /// 資產財產編號；未填為 `None`（對話框顯示「財產編號(描述)」，見 spec §4.3）。
    pub asset_property_no: Option<String>,
    pub asset_description: String,
    /// 資產廠牌；未填為 `None`。IP 清單指派對象第一行顯示「描述(廠牌 型號)」，
    /// 缺者省略（見 spec §4.3、票 16）。
    pub asset_brand: Option<String>,
    /// 資產型號；未填為 `None`（同 `asset_brand`）。
    pub asset_model: Option<String>,
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
    pub asset_property_no: Option<String>,
    pub asset_description: String,
    pub asset_brand: Option<String>,
    pub asset_model: Option<String>,
    pub asset_location: String,
}

impl ListedAssignment {
    /// 取出指派對象（資產＋介面），供 IP 列巢狀顯示。
    pub fn target(&self) -> IpAssignment {
        IpAssignment {
            asset_id: self.asset_id,
            asset_property_no: self.asset_property_no.clone(),
            asset_description: self.asset_description.clone(),
            asset_brand: self.asset_brand.clone(),
            asset_model: self.asset_model.clone(),
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
/// 重新指派（換介面即取消＋重新指派，見 GLOSSARY.md／ADR-0005）。
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
    .detail("asset_property_no", json!(target.asset_property_no))
    .detail("asset_description", json!(target.asset_description))
    .detail("asset_brand", json!(target.asset_brand))
    .detail("asset_model", json!(target.asset_model))
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

/// 依網段＋位址讀取指派；供 Kea 同步判斷異動前狀態（見 `docs/adr/0011`）。
pub async fn find(
    pool: &SqlitePool,
    subnet_id: i64,
    address: &str,
) -> sqlx::Result<Option<Assignment>> {
    Ok(fetch_by_address(pool, subnet_id, address)
        .await?
        .map(AssignmentRow::into_assignment))
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
                s.id AS asset_id, s.property_no AS asset_property_no,
                s.description AS asset_description,
                s.brand AS asset_brand, s.model AS asset_model,
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

/// 讀取多個資產（經由介面）的全部已指派位址，供資產清單「已指派 IP」欄
/// （見票 12）。
///
/// 以單一查詢取當頁資產的指派，回傳每資產一組已排序位址：v4 先、v6 後，
/// 同地址族依位址數值（見 spec §2.1）。無指派的資產不出現在回傳 map。
pub async fn list_for_assets(
    pool: &SqlitePool,
    asset_ids: &[i64],
) -> sqlx::Result<HashMap<i64, Vec<String>>> {
    if asset_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let mut query = QueryBuilder::new(
        "SELECT i.asset_id, a.address
           FROM ip_assignments a
           JOIN interfaces i ON i.id = a.interface_id
          WHERE i.asset_id IN (",
    );
    let mut separated = query.separated(", ");
    for asset_id in asset_ids {
        separated.push_bind(*asset_id);
    }
    separated.push_unseparated(")");

    let rows: Vec<(i64, String)> = query.build_query_as().fetch_all(pool).await?;

    let mut grouped: HashMap<i64, Vec<String>> = HashMap::new();
    for (asset_id, address) in rows {
        grouped.entry(asset_id).or_default().push(address);
    }
    for addresses in grouped.values_mut() {
        addresses.sort_by_key(|address| address_sort_key(address));
    }

    Ok(grouped)
}

/// 已指派位址的顯示排序鍵：v4 先、v6 後，同地址族依位址數值；無法解析者排最後。
///
/// 供本模組排序顯示序，亦供資產清單「已指派 IP」排序比較第一筆位址
/// （見 `assets::list_all_by_assigned_ips`、票 19）。
pub(crate) fn address_sort_key(address: &str) -> (u8, u128) {
    match address.parse::<IpAddr>() {
        Ok(IpAddr::V4(address)) => (0, u128::from(u32::from(address))),
        Ok(IpAddr::V6(address)) => (1, u128::from(address)),
        Err(_) => (2, 0),
    }
}

/// 資產匯出所需的網路欄位（MAC／IPv4／IPv6／hostname；見 spec §4）。
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ExportNetwork {
    pub mac: Option<String>,
    pub ipv4: Option<String>,
    pub ipv6: Option<String>,
    pub hostname: Option<String>,
}

/// 匯出查詢列：介面 LEFT JOIN 其指派（無指派時 address 為 NULL）。
#[derive(Debug, FromRow)]
struct ExportRow {
    asset_id: i64,
    interface_id: i64,
    mac: Option<String>,
    address: Option<String>,
    purpose: Option<String>,
    hostname: Option<String>,
}

/// 批次讀取多資產的匯出網路欄位（單一查詢，避免逐資產 N+1；見票 04）。
///
/// 回傳 map 僅含至少有一個介面的資產；無介面的資產由呼叫端視為全空。
pub async fn export_networks(
    pool: &SqlitePool,
    asset_ids: &[i64],
) -> sqlx::Result<HashMap<i64, ExportNetwork>> {
    if asset_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let mut query = QueryBuilder::new(
        "SELECT i.asset_id, i.id AS interface_id, i.mac,
                a.address, a.purpose, a.hostname
           FROM interfaces i
           LEFT JOIN ip_assignments a ON a.interface_id = i.id
          WHERE i.asset_id IN (",
    );
    let mut separated = query.separated(", ");
    for asset_id in asset_ids {
        separated.push_bind(*asset_id);
    }
    separated.push_unseparated(")");
    // 同介面的列連續，且依指派 id 升冪；選取只需逐列掃描（見 [`select_export_network`]）。
    query.push(" ORDER BY i.asset_id ASC, i.id ASC, a.id ASC");

    let rows: Vec<ExportRow> = query.build_query_as().fetch_all(pool).await?;

    let mut grouped: HashMap<i64, Vec<ExportRow>> = HashMap::new();
    for row in rows {
        grouped.entry(row.asset_id).or_default().push(row);
    }

    Ok(grouped
        .into_iter()
        .map(|(asset_id, rows)| (asset_id, select_export_network(&rows)))
        .collect())
}

/// 由一資產的介面列套用 spec §4 的匯出選取規則：
///
/// 1. 介面：有指派位址的介面中 id 最小者；若皆無指派，取 id 最小介面；
///    無介面 → MAC／IPv4／IPv6／hostname 全空。
/// 2. IPv4／IPv6：所選介面的指派中，同族位址數值最小者（v4 用 u32、v6 用 u128）；
///    無 → 空。
/// 3. hostname：僅當匯出的 IPv4 為 reservation 時帶該筆 hostname；其餘空。
///
/// 列須依 `interface_id` 升冪（同介面連續）；無法解析的位址略過不選。
fn select_export_network(rows: &[ExportRow]) -> ExportNetwork {
    // 依 interface_id 切出連續區段；取第一個有指派的介面，否則第一個介面。
    let mut chosen: Option<&[ExportRow]> = None;
    let mut fallback: Option<&[ExportRow]> = None;
    let mut start = 0;
    while start < rows.len() {
        let mut end = start;
        while end < rows.len() && rows[end].interface_id == rows[start].interface_id {
            end += 1;
        }
        let interface_rows = &rows[start..end];
        if fallback.is_none() {
            fallback = Some(interface_rows);
        }
        if chosen.is_none() && interface_rows.iter().any(|row| row.address.is_some()) {
            chosen = Some(interface_rows);
        }
        start = end;
    }

    let Some(interface_rows) = chosen.or(fallback) else {
        return ExportNetwork::default();
    };

    let mut ipv4: Option<(u32, &ExportRow)> = None;
    let mut ipv6: Option<(u128, &ExportRow)> = None;
    for row in interface_rows {
        let Some(address) = row.address.as_deref() else {
            continue;
        };
        match address.parse::<IpAddr>() {
            Ok(IpAddr::V4(address)) => {
                let value = u32::from(address);
                if ipv4.as_ref().is_none_or(|(best, _)| value < *best) {
                    ipv4 = Some((value, row));
                }
            }
            Ok(IpAddr::V6(address)) => {
                let value = u128::from(address);
                if ipv6.as_ref().is_none_or(|(best, _)| value < *best) {
                    ipv6 = Some((value, row));
                }
            }
            Err(_) => {}
        }
    }

    let hostname = ipv4
        .as_ref()
        .filter(|(_, row)| row.purpose.as_deref() == Some("reservation"))
        .and_then(|(_, row)| row.hostname.clone());

    ExportNetwork {
        mac: interface_rows[0].mac.clone(),
        ipv4: ipv4.as_ref().and_then(|(_, row)| row.address.clone()),
        ipv6: ipv6.as_ref().and_then(|(_, row)| row.address.clone()),
        hostname,
    }
}

/// 目前指派對象摘要（供匯入報告「已被指派／已登錄」訊息；見票 02）。
#[derive(Debug)]
pub(crate) struct AssignmentTarget {
    pub(crate) asset_description: String,
    pub(crate) asset_location: String,
    pub(crate) interface_name: Option<String>,
    pub(crate) mac: Option<String>,
}

/// 查詢某網段某位址目前的指派對象；未指派回傳 `None`。
///
/// 供匯入逐列驗證附上「位址已被指派」的目前指派對象（比照 ADR-0007 提示資訊樣式）。
pub(crate) async fn target_for_address(
    pool: &SqlitePool,
    subnet_id: i64,
    address: &str,
) -> sqlx::Result<Option<AssignmentTarget>> {
    let Some(row) = fetch_by_address(pool, subnet_id, address).await? else {
        return Ok(None);
    };
    let target = fetch_target(pool, row.interface_id).await?;
    Ok(target.map(|target| AssignmentTarget {
        asset_description: target.asset_description,
        asset_location: target.asset_location,
        interface_name: target.interface_name,
        mac: target.mac,
    }))
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
/// 供匯入標示錯誤原因（見票 02）。
pub(crate) fn is_in_pool(subnet: &Subnet, address: Ipv4Addr) -> Result<bool, ApiError> {
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
        "SELECT s.id AS asset_id, s.property_no AS asset_property_no,
                s.description AS asset_description, s.location AS asset_location,
                s.brand AS asset_brand, s.model AS asset_model,
                i.id AS interface_id, i.name AS interface_name, i.mac
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

    #[test]
    fn address_sort_key_orders_v4_before_v6_and_numerically() {
        let mut addresses = vec![
            "fd00:1::5".to_string(),
            "10.9.0.2".to_string(),
            "fd00::a".to_string(),
            "10.0.0.10".to_string(),
            "10.0.0.2".to_string(),
        ];
        addresses.sort_by_key(|address| address_sort_key(address));
        assert_eq!(
            addresses,
            vec!["10.0.0.2", "10.0.0.10", "10.9.0.2", "fd00::a", "fd00:1::5"],
            "v4 先、v6 後；同族依數值"
        );
    }

    /// 測試用匯出列；`address` 為 `None` 代表該介面無此指派。
    fn export_row(
        interface_id: i64,
        mac: Option<&str>,
        address: Option<&str>,
        purpose: Option<&str>,
        hostname: Option<&str>,
    ) -> ExportRow {
        ExportRow {
            asset_id: 1,
            interface_id,
            mac: mac.map(str::to_string),
            address: address.map(str::to_string),
            purpose: purpose.map(str::to_string),
            hostname: hostname.map(str::to_string),
        }
    }

    #[test]
    fn export_network_prefers_first_interface_with_assignments() {
        // 第一介面無指派、第二介面有 → 選第二（含其 MAC）。
        let rows = vec![
            export_row(1, Some("aa:bb:cc:dd:ee:01"), None, None, None),
            export_row(
                2,
                Some("aa:bb:cc:dd:ee:02"),
                Some("10.0.0.5"),
                Some("static"),
                None,
            ),
        ];
        let network = select_export_network(&rows);
        assert_eq!(network.mac.as_deref(), Some("aa:bb:cc:dd:ee:02"));
        assert_eq!(network.ipv4.as_deref(), Some("10.0.0.5"));
        assert_eq!(network.ipv6, None);
    }

    #[test]
    fn export_network_falls_back_to_first_interface_without_assignments() {
        let rows = vec![
            export_row(1, Some("aa:bb:cc:dd:ee:01"), None, None, None),
            export_row(2, Some("aa:bb:cc:dd:ee:02"), None, None, None),
        ];
        let network = select_export_network(&rows);
        assert_eq!(
            network.mac.as_deref(),
            Some("aa:bb:cc:dd:ee:01"),
            "取 id 最小介面"
        );
        assert_eq!(network.ipv4, None);
        assert_eq!(network.ipv6, None);
        assert_eq!(network.hostname, None);
    }

    #[test]
    fn export_network_without_interfaces_is_empty() {
        assert_eq!(select_export_network(&[]), ExportNetwork::default());
    }

    #[test]
    fn export_network_picks_numerically_smallest_address_per_family() {
        let rows = vec![
            export_row(
                1,
                Some("aa:bb:cc:dd:ee:01"),
                Some("10.0.0.10"),
                Some("static"),
                None,
            ),
            export_row(
                1,
                Some("aa:bb:cc:dd:ee:01"),
                Some("10.0.0.2"),
                Some("static"),
                None,
            ),
            export_row(
                1,
                Some("aa:bb:cc:dd:ee:01"),
                Some("fd00::a"),
                Some("static"),
                None,
            ),
            export_row(
                1,
                Some("aa:bb:cc:dd:ee:01"),
                Some("fd00::2"),
                Some("static"),
                None,
            ),
        ];
        let network = select_export_network(&rows);
        assert_eq!(
            network.ipv4.as_deref(),
            Some("10.0.0.2"),
            "v4 依數值而非字串"
        );
        assert_eq!(
            network.ipv6.as_deref(),
            Some("fd00::2"),
            "v6 依數值而非字串"
        );
    }

    #[test]
    fn export_network_hostname_only_from_reservation_ipv4() {
        // 匯出的 IPv4 為 reservation → 帶該筆 hostname。
        let rows = vec![export_row(
            1,
            Some("aa:bb:cc:dd:ee:01"),
            Some("10.0.0.2"),
            Some("reservation"),
            Some("pc-1"),
        )];
        assert_eq!(
            select_export_network(&rows).hostname.as_deref(),
            Some("pc-1")
        );

        // 匯出的 IPv4 為 static（數值較小）→ hostname 空，即使保留另有 hostname。
        let rows = vec![
            export_row(
                1,
                Some("aa:bb:cc:dd:ee:01"),
                Some("10.0.0.1"),
                Some("static"),
                None,
            ),
            export_row(
                1,
                Some("aa:bb:cc:dd:ee:01"),
                Some("10.0.0.2"),
                Some("reservation"),
                Some("pc-1"),
            ),
        ];
        let network = select_export_network(&rows);
        assert_eq!(network.ipv4.as_deref(), Some("10.0.0.1"));
        assert_eq!(network.hostname, None, "非保留不帶 hostname");

        // 同時有 v4 保留與 v6 指派 → 兩族皆帶，hostname 取自 v4 保留。
        let rows = vec![
            export_row(
                1,
                Some("aa:bb:cc:dd:ee:01"),
                Some("10.0.0.9"),
                Some("reservation"),
                Some("pc-9"),
            ),
            export_row(
                1,
                Some("aa:bb:cc:dd:ee:01"),
                Some("fd00::9"),
                Some("static"),
                None,
            ),
        ];
        let network = select_export_network(&rows);
        assert_eq!(network.ipv6.as_deref(), Some("fd00::9"));
        assert_eq!(network.hostname.as_deref(), Some("pc-9"));
    }
}
