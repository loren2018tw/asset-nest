//! 指派（Assignment）領域模組：指派／改用途、v6 登錄、取消與指派對象查詢。
//!
//! 詞彙依 `CONTEXT.md`；規則見 `.scratch/asset-ip-management/spec.md` §2.4、§3.1、
//! §4.3、§5，決策見 ADR-0005（指派以 Interface 為對象）與 ADR-0006（結構錯誤阻擋）。
//! v6 採登錄制：新增即指派、用途固定 static；語意衝突標記為票 07。

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use ipnet::IpNet;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{FromRow, SqlitePool};

use crate::api::ApiError;
use crate::assets::optional_text;
use crate::interfaces;
use crate::ips::HostRange;
use crate::subnets::Subnet;

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
/// v4：位址須為 host（扣除 network/broadcast）且不在 pool 內；
/// v6：登錄制，位址須落在 CIDR 內（含 network 位址；無 host 扣除概念），
/// 用途固定 static。同一網段同一位址已指派給其他介面時阻擋，須先取消再
/// 重新指派（換介面即取消＋重新指派，見 CONTEXT.md／ADR-0005）。
pub async fn assign(
    pool: &SqlitePool,
    subnet: &Subnet,
    address: IpAddr,
    valid: ValidAssignment,
) -> Result<Assignment, ApiError> {
    validate_address(subnet, address, valid.purpose)?;

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
fn validate_address(subnet: &Subnet, address: IpAddr, purpose: &str) -> Result<(), ApiError> {
    let network = parse_network(&subnet.cidr)?;

    match (network, address) {
        (IpNet::V4(network), IpAddr::V4(address)) => {
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
        (IpNet::V6(network), IpAddr::V6(address)) => {
            if !network.contains(&address) {
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
