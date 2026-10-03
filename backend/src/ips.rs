//! IP 位址（IpAddress）領域模組：v4 位址枚舉、pool/gateway 標示與分頁瀏覽。
//!
//! 詞彙依 `CONTEXT.md`；規則見 `.scratch/asset-ip-management/spec.md` §2.4、§4.3、§7。
//! 位址不建表，一律由 Subnet 範圍伺服器端推導。本票（04）為唯讀清單：
//! 指派為票 05、v6 登錄制為票 06、衝突標記為票 07。

use std::net::Ipv4Addr;

use ipnet::{IpNet, Ipv4Net};
use serde::Serialize;

use crate::api::ApiError;
use crate::subnets::Subnet;

/// v4 網段的 host 位址範圍；端點皆含。
///
/// 扣除 network/broadcast；`/31`、`/32` 全數列出（見 spec §7）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostRange {
    first: u32,
    last: u32,
}

impl HostRange {
    /// 由 v4 CIDR 推導 host 範圍。
    pub fn of(network: &Ipv4Net) -> Self {
        let base = u64::from(network.network().to_bits());
        let size = 1u64 << (32 - network.prefix_len());
        match network.prefix_len() {
            // /31（RFC 3021 點對點）與 /32：全部位址皆為 host。
            31 | 32 => Self {
                first: base as u32,
                last: (base + size - 1) as u32,
            },
            // 其餘前綴：扣除 network 與 broadcast。
            _ => Self {
                first: (base + 1) as u32,
                last: (base + size - 2) as u32,
            },
        }
    }

    /// host 位址總數。
    pub fn count(&self) -> u64 {
        u64::from(self.last) - u64::from(self.first) + 1
    }

    /// 位址是否在範圍內。
    pub fn contains(&self, address: Ipv4Addr) -> bool {
        (self.first..=self.last).contains(&address.to_bits())
    }

    /// 數值排序第 `offset` 個位址（0 起算）；超出範圍回傳 `None`。
    pub fn nth(&self, offset: u64) -> Option<Ipv4Addr> {
        let value = u64::from(self.first).checked_add(offset)?;
        (value <= u64::from(self.last)).then(|| Ipv4Addr::from(value as u32))
    }

    /// 依數值升冪走訪所有 host 位址。
    pub fn iter(&self) -> impl Iterator<Item = Ipv4Addr> + '_ {
        (u64::from(self.first)..=u64::from(self.last)).map(|value| Ipv4Addr::from(value as u32))
    }
}

/// 解析後的 pool 範圍；端點皆含。
#[derive(Debug, Clone, Copy)]
struct PoolRange {
    start: Ipv4Addr,
    end: Ipv4Addr,
}

impl PoolRange {
    fn contains(&self, address: Ipv4Addr) -> bool {
        self.start <= address && address <= self.end
    }
}

/// IP 清單的搜尋與分頁條件（見 spec §4.3、§5）。
#[derive(Debug, Default)]
pub struct IpFilter {
    /// 關鍵字：可解析為完整位址時精確比對，否則對位址文字做子字串比對。
    pub q: Option<String>,
    pub page: i64,
    pub per_page: i64,
}

/// API 回傳的 IP 列。
#[derive(Debug, Serialize)]
pub struct IpEntry {
    pub address: Ipv4Addr,
    /// 落在 DHCP 位址池內；池內位址不可指派（見 CONTEXT.md）。
    pub in_pool: bool,
    /// 是否為網段設定的 gateway（僅標記，仍可被指派）。
    pub is_gateway: bool,
    /// 狀態：本票為 `available`（可用）或 `in_pool`（池內）；
    /// 票 05 起指派後為 `static`／`reservation`。
    pub status: &'static str,
    /// 指派用途；未指派為 `null`（預留欄位，票 05 實作）。
    pub purpose: Option<&'static str>,
    /// 衝突標記；本票尚無指派資料，恆為空（預留欄位，票 07 實作）。
    pub conflicts: Vec<&'static str>,
}

/// 由網段推導一頁 IP 列；回傳（當頁列、符合總數）。
///
/// v6 網段尚未支援，回傳 `not_implemented`（票 06）。列以數值升冪排序，
/// 枚舉順序即排序；無關鍵字時總數與當頁皆以算術位移取得，不需掃描全部位址。
pub fn list(subnet: &Subnet, filter: &IpFilter) -> Result<(Vec<IpEntry>, u64), ApiError> {
    let network = parse_network(&subnet.cidr)?;
    let pools = parse_pools(subnet)?;
    let gateway = parse_gateway(subnet.gateway.as_deref())?;
    let range = HostRange::of(&network);

    let per_page = u64::try_from(filter.per_page).unwrap_or(1).max(1);
    let offset = u64::try_from(filter.page.max(1) - 1)
        .unwrap_or(0)
        .saturating_mul(per_page);
    let query = filter.q.as_deref().map(str::trim).filter(|q| !q.is_empty());

    let (items, total) = match query {
        Some(q) => match q.parse::<Ipv4Addr>() {
            // 完整位址：精確比對，最多一筆。
            Ok(exact) => {
                let total = u64::from(range.contains(exact));
                let items = if offset == 0 && total == 1 {
                    vec![entry(exact, &pools, gateway)]
                } else {
                    Vec::new()
                };
                (items, total)
            }
            // 部分關鍵字：線性掃描位址文字；極大前綴（如 /8 以下）成本較高，
            // 實務網段規模（/16–/32）可忽略（見票檔註記）。
            Err(_) => {
                let mut total: u64 = 0;
                let mut items = Vec::new();
                for address in range.iter() {
                    if !address.to_string().contains(q) {
                        continue;
                    }
                    if total >= offset && (items.len() as u64) < per_page {
                        items.push(entry(address, &pools, gateway));
                    }
                    total += 1;
                }
                (items, total)
            }
        },
        None => {
            let total = range.count();
            let end = offset.saturating_add(per_page).min(total);
            let items = (offset..end)
                .filter_map(|index| range.nth(index))
                .map(|address| entry(address, &pools, gateway))
                .collect();
            (items, total)
        }
    };

    Ok((items, total))
}

/// 建立一列；本票尚無指派資料，狀態僅有「可用」與「池內」（票 05 擴充）。
fn entry(address: Ipv4Addr, pools: &[PoolRange], gateway: Option<Ipv4Addr>) -> IpEntry {
    let in_pool = pools.iter().any(|pool| pool.contains(address));
    IpEntry {
        address,
        in_pool,
        is_gateway: gateway == Some(address),
        status: if in_pool { "in_pool" } else { "available" },
        purpose: None,
        conflicts: Vec::new(),
    }
}

/// 解析網段 CIDR；v6 尚未支援（票 06），資料異常回傳內部錯誤。
fn parse_network(cidr: &str) -> Result<Ipv4Net, ApiError> {
    match cidr.parse::<IpNet>() {
        Ok(IpNet::V4(network)) => Ok(network),
        Ok(IpNet::V6(_)) => Err(ApiError::not_implemented(
            "IPv6 網段的 IP 清單尚未支援（見票 06）",
        )),
        Err(_) => Err(ApiError::internal("網段 CIDR 格式錯誤", cidr)),
    }
}

/// 解析網段的所有 pool 範圍；資料庫內容經結構驗證，格式異常視為內部錯誤。
fn parse_pools(subnet: &Subnet) -> Result<Vec<PoolRange>, ApiError> {
    subnet
        .pools
        .iter()
        .map(|pool| {
            Ok(PoolRange {
                start: parse_stored_address(&pool.start_ip)?,
                end: parse_stored_address(&pool.end_ip)?,
            })
        })
        .collect()
}

/// 解析 gateway；未設定為 `None`。
fn parse_gateway(value: Option<&str>) -> Result<Option<Ipv4Addr>, ApiError> {
    value.map(parse_stored_address).transpose()
}

/// 解析資料庫中已驗證過的 v4 位址文字。
fn parse_stored_address(text: &str) -> Result<Ipv4Addr, ApiError> {
    text.parse()
        .map_err(|error| ApiError::internal("位址格式錯誤", error))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::subnets::Pool;

    /// 測試用位址。
    fn addr(text: &str) -> Ipv4Addr {
        text.parse().expect("合法位址")
    }

    /// 測試用網段；pools 以（起點、終點）表示。
    fn subnet(cidr: &str, gateway: Option<&str>, pools: &[(&str, &str)]) -> Subnet {
        Subnet {
            id: 1,
            cidr: cidr.to_string(),
            name: None,
            note: None,
            gateway: gateway.map(str::to_string),
            kea_subnet_id: None,
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

    /// 測試用篩選條件。
    fn filter(q: Option<&str>, page: i64, per_page: i64) -> IpFilter {
        IpFilter {
            q: q.map(str::to_string),
            page,
            per_page,
        }
    }

    /// 取出列中的位址文字。
    fn addresses(items: &[IpEntry]) -> Vec<String> {
        items
            .iter()
            .map(|entry| entry.address.to_string())
            .collect()
    }

    #[test]
    fn host_range_excludes_network_and_broadcast() {
        let range = HostRange::of(&"10.0.0.0/24".parse().expect("合法 CIDR"));
        assert_eq!(range.count(), 254);
        assert_eq!(range.nth(0), Some(addr("10.0.0.1")));
        assert_eq!(range.nth(253), Some(addr("10.0.0.254")));
        assert_eq!(range.nth(254), None, "超出範圍");
        assert!(!range.contains(addr("10.0.0.0")), "network 不在範圍內");
        assert!(!range.contains(addr("10.0.0.255")), "broadcast 不在範圍內");
    }

    #[test]
    fn host_range_lists_all_for_slash31_and_slash32() {
        let range = HostRange::of(&"10.0.1.0/31".parse().expect("合法 CIDR"));
        assert_eq!(range.count(), 2, "/31 全數列出");
        assert_eq!(range.nth(0), Some(addr("10.0.1.0")));
        assert_eq!(range.nth(1), Some(addr("10.0.1.1")));
        assert_eq!(range.nth(2), None);

        let range = HostRange::of(&"10.0.2.7/32".parse().expect("合法 CIDR"));
        assert_eq!(range.count(), 1, "/32 全數列出");
        assert_eq!(range.nth(0), Some(addr("10.0.2.7")));
        assert_eq!(range.nth(1), None);
    }

    #[test]
    fn host_range_supports_any_prefix() {
        let range = HostRange::of(&"10.0.0.0/30".parse().expect("合法 CIDR"));
        assert_eq!(range.count(), 2);
        assert_eq!(range.nth(0), Some(addr("10.0.0.1")));
        assert_eq!(range.nth(1), Some(addr("10.0.0.2")));

        let range = HostRange::of(&"0.0.0.0/0".parse().expect("合法 CIDR"));
        assert_eq!(range.count(), u64::from(u32::MAX) - 1);
        assert_eq!(range.nth(0), Some(addr("0.0.0.1")));
        assert_eq!(range.nth(range.count() - 1), Some(addr("255.255.255.254")));
    }

    #[test]
    fn list_paginates_in_numeric_order() {
        let subnet = subnet("10.0.0.0/29", None, &[]);

        let (items, total) = list(&subnet, &filter(None, 1, 2)).expect("推導成功");
        assert_eq!(total, 6);
        assert_eq!(addresses(&items), ["10.0.0.1", "10.0.0.2"]);

        let (items, total) = list(&subnet, &filter(None, 2, 2)).expect("推導成功");
        assert_eq!(total, 6);
        assert_eq!(addresses(&items), ["10.0.0.3", "10.0.0.4"]);

        let (items, _) = list(&subnet, &filter(None, 4, 2)).expect("推導成功");
        assert!(items.is_empty(), "超出範圍的頁為空");
    }

    #[test]
    fn pool_and_gateway_are_marked() {
        let subnet = subnet("10.0.0.0/29", Some("10.0.0.1"), &[("10.0.0.2", "10.0.0.3")]);
        let (items, total) = list(&subnet, &filter(None, 1, 50)).expect("推導成功");
        assert_eq!(total, 6);

        let entry = |address: &str| {
            items
                .iter()
                .find(|entry| entry.address == addr(address))
                .expect("位址存在")
        };

        assert!(entry("10.0.0.1").is_gateway, "gateway 標記");
        assert!(!entry("10.0.0.1").in_pool);
        assert_eq!(entry("10.0.0.1").status, "available");

        assert!(entry("10.0.0.2").in_pool, "pool 內標記");
        assert!(!entry("10.0.0.2").is_gateway);
        assert_eq!(entry("10.0.0.2").status, "in_pool");
        assert_eq!(entry("10.0.0.2").purpose, None, "票 05 前無指派用途");
        assert!(entry("10.0.0.2").conflicts.is_empty(), "票 07 前無衝突");

        assert_eq!(entry("10.0.0.4").status, "available");
    }

    #[test]
    fn search_matches_exact_address_or_substring() {
        let subnet = subnet("10.0.0.0/28", None, &[]);

        // 完整位址：精確比對，僅一筆（不誤含 .100 等子字串候選）。
        let (items, total) = list(&subnet, &filter(Some("10.0.0.10"), 1, 50)).expect("推導成功");
        assert_eq!(total, 1);
        assert_eq!(addresses(&items), ["10.0.0.10"]);

        // 部分關鍵字：位址文字子字串比對（.1x 的六個位址），維持數值排序。
        let (items, total) = list(&subnet, &filter(Some(".1"), 1, 50)).expect("推導成功");
        assert_eq!(total, 6);
        assert_eq!(
            addresses(&items),
            [
                "10.0.0.1",
                "10.0.0.10",
                "10.0.0.11",
                "10.0.0.12",
                "10.0.0.13",
                "10.0.0.14"
            ]
        );

        // 子字串搜尋亦分頁（第 2 頁、每頁 2 筆）。
        let (items, total) = list(&subnet, &filter(Some(".1"), 2, 2)).expect("推導成功");
        assert_eq!(total, 6);
        assert_eq!(addresses(&items), ["10.0.0.11", "10.0.0.12"]);

        // 完整位址不在範圍內：空結果。
        let (items, total) = list(&subnet, &filter(Some("10.0.1.1"), 1, 50)).expect("推導成功");
        assert_eq!(total, 0);
        assert!(items.is_empty());
    }
}
