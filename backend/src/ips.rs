//! IP 位址（IpAddress）領域模組：v4 位址枚舉、v6 登錄制清單、pool/gateway
//! 標示、衝突標記與分頁瀏覽。
//!
//! 詞彙依 `CONTEXT.md`；規則見 `.scratch/asset-ip-management/spec.md` §2.4、§4.3、§7。
//! v4 位址由 Subnet 範圍伺服器端推導；v6 採登錄制，僅列出已指派（登錄）位址，
//! 不枚舉空閒位址。指派為票 05、v6 登錄制為票 06、衝突標記為票 07
//! （偵測集中於 [`crate::conflicts`]）。

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use ipnet::{IpNet, Ipv4Net};
use serde::Serialize;

use crate::api::ApiError;
use crate::assignments::{IpAssignment, ListedAssignment};
use crate::conflicts;
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
    /// 關鍵字：可解析為完整位址時精確比對，否則對位址文字與指派對象
    /// （資產描述／位置／介面名稱／MAC）做子字串比對。
    pub q: Option<String>,
    /// 狀態／用途篩選；未提供即全部。
    pub status: Option<IpStatusFilter>,
    pub page: i64,
    pub per_page: i64,
}

/// 狀態／用途篩選值；未知值由 API 層回 400。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IpStatusFilter {
    Available,
    InPool,
    Static,
    Reservation,
}

impl IpStatusFilter {
    /// 解析查詢參數；未知值回傳 `None`。
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "available" => Some(Self::Available),
            "in_pool" => Some(Self::InPool),
            "static" => Some(Self::Static),
            "reservation" => Some(Self::Reservation),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::InPool => "in_pool",
            Self::Static => "static",
            Self::Reservation => "reservation",
        }
    }
}

/// API 回傳的 IP 列。
#[derive(Debug, Serialize)]
pub struct IpEntry {
    /// v4 或 v6 位址（JSON 序列化為文字）。
    pub address: IpAddr,
    /// 落在 DHCP 位址池內；池內位址不可指派（v6 恆為 `false`，見 CONTEXT.md）。
    pub in_pool: bool,
    /// 是否為網段設定的 gateway（僅標記，仍可被指派）。
    pub is_gateway: bool,
    /// 狀態：`available`（可用）、`in_pool`（池內）、
    /// `static`（手動設定）或 `reservation`（保留）。
    pub status: &'static str,
    /// 指派用途；未指派為 `null`。
    pub purpose: Option<&'static str>,
    /// 指派對象（資產描述／位置、介面名稱／MAC）；未指派為 `null`。
    pub assignment: Option<IpAssignment>,
    /// 衝突標記：命中的語意規則代碼（見 [`crate::conflicts`]；
    /// 依固定順序排列，僅標記、不阻擋）。
    pub conflicts: Vec<&'static str>,
}

/// 由網段推導一頁 IP 列；回傳（當頁列、符合總數）。
///
/// v4 枚舉全部 host 位址，並聯集出界指派列（網段縮小造成；見票 07）；
/// v6 為登錄制，僅列出已指派（登錄）位址。
/// 搜尋、狀態篩選與分頁皆為伺服器端。`assignments` 為該網段的指派列（見票 05）；
/// 每列 `conflicts` 由 [`crate::conflicts`] 即時偵測填入。
pub fn list(
    subnet: &Subnet,
    filter: &IpFilter,
    assignments: &[ListedAssignment],
) -> Result<(Vec<IpEntry>, u64), ApiError> {
    match parse_network(&subnet.cidr)? {
        IpNet::V4(network) => list_v4(subnet, &network, filter, assignments),
        IpNet::V6(_) => list_v6(subnet, filter, assignments),
    }
}

/// v4：由網段範圍枚舉一頁 IP 列；回傳（當頁列、符合總數）。
///
/// 列以數值升冪排序，枚舉順序即排序；無關鍵字、無狀態篩選時總數與當頁
/// 皆以算術位移取得，不需掃描全部位址。
fn list_v4(
    subnet: &Subnet,
    network: &Ipv4Net,
    filter: &IpFilter,
    assignments: &[ListedAssignment],
) -> Result<(Vec<IpEntry>, u64), ApiError> {
    let pools = parse_pools(subnet)?;
    let gateway = parse_gateway_v4(subnet.gateway.as_deref())?;
    let range = HostRange::of(network);
    let conflicts = conflicts::by_address(subnet, assignments)?;

    // 指派資料以位址文字索引；用途經資料庫 CHECK 驗證，異常視為內部錯誤。
    let mut by_address: HashMap<&str, &ListedAssignment> =
        HashMap::with_capacity(assignments.len());
    // 出界指派（如網段縮小造成）：不在 host 範圍仍須出現於清單（見票 07），
    // 以「host 範圍列 ∪ 指派列」呈現，並由衝突標記說明。
    let mut extras: Vec<Ipv4Addr> = Vec::new();
    for assignment in assignments {
        if assignment.purpose != "static" && assignment.purpose != "reservation" {
            return Err(ApiError::internal("指派用途資料異常", &assignment.purpose));
        }
        by_address.insert(&assignment.address, assignment);

        let address: Ipv4Addr = assignment
            .address
            .parse()
            .map_err(|error| ApiError::internal("指派位址格式錯誤", error))?;
        if !range.contains(address) {
            extras.push(address);
        }
    }
    extras.sort_unstable();

    let per_page = u64::try_from(filter.per_page).unwrap_or(1).max(1);
    let offset = u64::try_from(filter.page.max(1) - 1)
        .unwrap_or(0)
        .saturating_mul(per_page);
    let query = filter.q.as_deref().map(str::trim).filter(|q| !q.is_empty());

    // 完整位址且無狀態篩選：精確比對，最多一筆（不掃描整個網段）。
    if filter.status.is_none() {
        if let Some(q) = query {
            if let Ok(exact) = q.parse::<Ipv4Addr>() {
                let total =
                    u64::from(range.contains(exact) || extras.binary_search(&exact).is_ok());
                let items = if offset == 0 && total == 1 {
                    vec![entry_v4(exact, &pools, gateway, &by_address, &conflicts)]
                } else {
                    Vec::new()
                };
                return Ok((items, total));
            }
        }
    }

    // 無條件且無出界列：以算術位移取得當頁（比照票 04）。
    if query.is_none() && filter.status.is_none() && extras.is_empty() {
        let total = range.count();
        let end = offset.saturating_add(per_page).min(total);
        let items = (offset..end)
            .filter_map(|index| range.nth(index))
            .map(|address| entry_v4(address, &pools, gateway, &by_address, &conflicts))
            .collect();
        return Ok((items, total));
    }

    // 一般情況：線性掃描位址並依關鍵字／狀態過濾，出界指派列以數值順序合併；
    // 極大前綴成本較高，實務網段規模（/16–/32）可忽略（見票 04 註記）。
    let mut scanner = V4Scanner {
        pools: &pools,
        gateway,
        assignments: &by_address,
        conflicts: &conflicts,
        status: filter.status,
        query_lower: query.map(str::to_lowercase),
        offset,
        per_page,
        total: 0,
        items: Vec::new(),
    };
    let mut extras_iter = extras.iter().peekable();
    for address in range.iter() {
        while extras_iter.peek().is_some_and(|extra| **extra < address) {
            if let Some(extra) = extras_iter.next() {
                scanner.consider(*extra);
            }
        }
        scanner.consider(address);
    }
    for extra in extras_iter {
        scanner.consider(*extra);
    }

    Ok((scanner.items, scanner.total))
}

/// v4 清單掃描：合併 host 範圍與出界指派列，依序套用篩選與分頁。
struct V4Scanner<'a> {
    pools: &'a [PoolRange],
    gateway: Option<Ipv4Addr>,
    assignments: &'a HashMap<&'a str, &'a ListedAssignment>,
    conflicts: &'a HashMap<String, Vec<&'static str>>,
    status: Option<IpStatusFilter>,
    query_lower: Option<String>,
    offset: u64,
    per_page: u64,
    total: u64,
    items: Vec<IpEntry>,
}

impl V4Scanner<'_> {
    /// 考慮一個位址：符合篩選時計入總數，並在當頁範圍內收列。
    fn consider(&mut self, address: Ipv4Addr) {
        let text = address.to_string();
        let assignment = self.assignments.get(text.as_str()).copied();

        if let Some(status) = self.status {
            if status_of(self.pools, address, assignment) != status.as_str() {
                return;
            }
        }
        if let Some(query) = &self.query_lower {
            if !matches_query(assignment, &text, query) {
                return;
            }
        }

        if self.total >= self.offset && (self.items.len() as u64) < self.per_page {
            self.items.push(entry_v4(
                address,
                self.pools,
                self.gateway,
                self.assignments,
                self.conflicts,
            ));
        }
        self.total += 1;
    }
}

/// 建立一列 v4；狀態由指派資料推導，未指派且不在 pool 內為「可用」。
fn entry_v4(
    address: Ipv4Addr,
    pools: &[PoolRange],
    gateway: Option<Ipv4Addr>,
    assignments: &HashMap<&str, &ListedAssignment>,
    conflicts: &HashMap<String, Vec<&'static str>>,
) -> IpEntry {
    let in_pool = pools.iter().any(|pool| pool.contains(address));
    let text = address.to_string();
    let assignment = assignments.get(text.as_str()).copied();
    IpEntry {
        address: IpAddr::V4(address),
        in_pool,
        is_gateway: gateway == Some(address),
        status: status_of(pools, address, assignment),
        purpose: assignment.map(|item| purpose_str(&item.purpose)),
        assignment: assignment.map(ListedAssignment::target),
        conflicts: assignment
            .and_then(|item| conflicts.get(&item.address))
            .cloned()
            .unwrap_or_default(),
    }
}

/// v6：登錄制，僅列出該網段已登錄（有指派）的位址；無 pool、無空閒列。
///
/// 列以數值（u128）升冪排序；支援關鍵字與狀態篩選、伺服器端分頁。
/// 狀態恆為 `static`（v6 用途固定手動）；`available`、`in_pool`、
/// `reservation` 篩選皆回空集合。登錄位址即使因網段縮小而出界仍會列出，
/// 並以 IpOutOfSubnet 衝突標記呈現（見票 06、07）。
fn list_v6(
    subnet: &Subnet,
    filter: &IpFilter,
    assignments: &[ListedAssignment],
) -> Result<(Vec<IpEntry>, u64), ApiError> {
    let gateway = parse_gateway_v6(subnet.gateway.as_deref())?;
    let conflicts = conflicts::by_address(subnet, assignments)?;

    // 指派資料即登錄清單；用途經資料庫 CHECK 驗證，v6 恆為 static，
    // 異常（如直接寫入 reservation）視為內部錯誤。
    let mut registered: Vec<(&ListedAssignment, Ipv6Addr)> = Vec::with_capacity(assignments.len());
    for assignment in assignments {
        if assignment.purpose != "static" {
            return Err(ApiError::internal(
                "v6 指派用途資料異常",
                &assignment.purpose,
            ));
        }
        let address: Ipv6Addr = assignment
            .address
            .parse()
            .map_err(|error| ApiError::internal("v6 位址格式錯誤", error))?;
        registered.push((assignment, address));
    }
    registered.sort_by_key(|(_, address)| u128::from(*address));

    let per_page = u64::try_from(filter.per_page).unwrap_or(1).max(1);
    let offset = u64::try_from(filter.page.max(1) - 1)
        .unwrap_or(0)
        .saturating_mul(per_page);
    let query = filter.q.as_deref().map(str::trim).filter(|q| !q.is_empty());
    // 完整位址：解析後以數值精確比對（容許不同壓縮寫法）。
    let query_address: Option<Ipv6Addr> = query.and_then(|q| q.parse().ok());
    let query_lower = query.map(str::to_lowercase);

    let mut total: u64 = 0;
    let mut items = Vec::new();
    for (assignment, address) in registered {
        if filter
            .status
            .is_some_and(|status| status != IpStatusFilter::Static)
        {
            continue;
        }
        if let Some(parsed) = query_address {
            if address != parsed {
                continue;
            }
        } else if let Some(query) = &query_lower {
            let text = address.to_string();
            if !matches_query(Some(assignment), &text, query) {
                continue;
            }
        }
        if total >= offset && (items.len() as u64) < per_page {
            items.push(entry_v6(address, gateway, assignment, &conflicts));
        }
        total += 1;
    }

    Ok((items, total))
}

/// 建立一列 v6 登錄位址；狀態恆為「手動設定」。
fn entry_v6(
    address: Ipv6Addr,
    gateway: Option<Ipv6Addr>,
    assignment: &ListedAssignment,
    conflicts: &HashMap<String, Vec<&'static str>>,
) -> IpEntry {
    IpEntry {
        address: IpAddr::V6(address),
        in_pool: false,
        is_gateway: gateway == Some(address),
        status: "static",
        purpose: Some("static"),
        assignment: Some(assignment.target()),
        conflicts: conflicts
            .get(&assignment.address)
            .cloned()
            .unwrap_or_default(),
    }
}

/// 列狀態：已指派以用途為準，未指派則區分池內與可用。
fn status_of(
    pools: &[PoolRange],
    address: Ipv4Addr,
    assignment: Option<&ListedAssignment>,
) -> &'static str {
    match assignment {
        Some(item) => purpose_str(&item.purpose),
        None if pools.iter().any(|pool| pool.contains(address)) => "in_pool",
        None => "available",
    }
}

/// 指派用途轉狀態字串；呼叫端已驗證值域。
fn purpose_str(purpose: &str) -> &'static str {
    if purpose == "static" {
        "static"
    } else {
        "reservation"
    }
}

/// 關鍵字比對：位址文字或指派對象（資產描述／位置／介面名稱／MAC）子字串，
/// 皆不分大小寫（見 spec §4.3）。
fn matches_query(
    assignment: Option<&ListedAssignment>,
    address_text: &str,
    query_lower: &str,
) -> bool {
    if address_text.to_lowercase().contains(query_lower) {
        return true;
    }

    let Some(assignment) = assignment else {
        return false;
    };

    assignment
        .asset_description
        .to_lowercase()
        .contains(query_lower)
        || assignment
            .asset_location
            .to_lowercase()
            .contains(query_lower)
        || assignment
            .interface_name
            .as_deref()
            .is_some_and(|name| name.to_lowercase().contains(query_lower))
        || assignment
            .mac
            .as_deref()
            .is_some_and(|mac| mac.to_lowercase().contains(query_lower))
}

/// 解析網段 CIDR（v4／v6）；資料異常回傳內部錯誤。
fn parse_network(cidr: &str) -> Result<IpNet, ApiError> {
    cidr.parse()
        .map_err(|error| ApiError::internal("網段 CIDR 格式錯誤", error))
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

/// 解析 v4 gateway；未設定為 `None`。
fn parse_gateway_v4(value: Option<&str>) -> Result<Option<Ipv4Addr>, ApiError> {
    value.map(parse_stored_address).transpose()
}

/// 解析 v6 gateway；未設定為 `None`。
fn parse_gateway_v6(value: Option<&str>) -> Result<Option<Ipv6Addr>, ApiError> {
    value
        .map(|text| {
            text.parse()
                .map_err(|error| ApiError::internal("gateway 格式錯誤", error))
        })
        .transpose()
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
            status: None,
            page,
            per_page,
        }
    }

    /// 測試用狀態篩選條件。
    fn status_filter(status: IpStatusFilter) -> IpFilter {
        IpFilter {
            q: None,
            status: Some(status),
            page: 1,
            per_page: 50,
        }
    }

    /// 無指派資料的呼叫捷徑（比照票 04 測試）。
    fn list_entries(subnet: &Subnet, filter: &IpFilter) -> Result<(Vec<IpEntry>, u64), ApiError> {
        list(subnet, filter, &[])
    }

    /// 測試用指派列（資產位置固定「機房 A」）。
    fn listed(
        address: &str,
        purpose: &str,
        description: &str,
        interface_name: Option<&str>,
        mac: Option<&str>,
    ) -> ListedAssignment {
        listed_at(address, purpose, description, "機房 A", interface_name, mac)
    }

    /// 測試用指派列（指定資產位置）。
    fn listed_at(
        address: &str,
        purpose: &str,
        description: &str,
        location: &str,
        interface_name: Option<&str>,
        mac: Option<&str>,
    ) -> ListedAssignment {
        ListedAssignment {
            address: address.to_string(),
            purpose: purpose.to_string(),
            hostname: None,
            interface_id: 1,
            interface_name: interface_name.map(str::to_string),
            mac: mac.map(str::to_string),
            asset_id: 1,
            asset_description: description.to_string(),
            asset_location: location.to_string(),
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

        let (items, total) = list_entries(&subnet, &filter(None, 1, 2)).expect("推導成功");
        assert_eq!(total, 6);
        assert_eq!(addresses(&items), ["10.0.0.1", "10.0.0.2"]);

        let (items, total) = list_entries(&subnet, &filter(None, 2, 2)).expect("推導成功");
        assert_eq!(total, 6);
        assert_eq!(addresses(&items), ["10.0.0.3", "10.0.0.4"]);

        let (items, _) = list_entries(&subnet, &filter(None, 4, 2)).expect("推導成功");
        assert!(items.is_empty(), "超出範圍的頁為空");
    }

    #[test]
    fn pool_and_gateway_are_marked() {
        let subnet = subnet("10.0.0.0/29", Some("10.0.0.1"), &[("10.0.0.2", "10.0.0.3")]);
        let (items, total) = list_entries(&subnet, &filter(None, 1, 50)).expect("推導成功");
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
        assert!(entry("10.0.0.2").conflicts.is_empty(), "無指派不標記衝突");

        assert_eq!(entry("10.0.0.4").status, "available");
    }

    #[test]
    fn search_matches_exact_address_or_substring() {
        let subnet = subnet("10.0.0.0/28", None, &[]);

        // 完整位址：精確比對，僅一筆（不誤含 .100 等子字串候選）。
        let (items, total) =
            list_entries(&subnet, &filter(Some("10.0.0.10"), 1, 50)).expect("推導成功");
        assert_eq!(total, 1);
        assert_eq!(addresses(&items), ["10.0.0.10"]);

        // 部分關鍵字：位址文字子字串比對（.1x 的六個位址），維持數值排序。
        let (items, total) = list_entries(&subnet, &filter(Some(".1"), 1, 50)).expect("推導成功");
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
        let (items, total) = list_entries(&subnet, &filter(Some(".1"), 2, 2)).expect("推導成功");
        assert_eq!(total, 6);
        assert_eq!(addresses(&items), ["10.0.0.11", "10.0.0.12"]);

        // 完整位址不在範圍內：空結果。
        let (items, total) =
            list_entries(&subnet, &filter(Some("10.0.1.1"), 1, 50)).expect("推導成功");
        assert_eq!(total, 0);
        assert!(items.is_empty());
    }

    #[test]
    fn assigned_rows_report_status_and_target() {
        // .6 落在 pool 內卻已指派：狀態以用途為準、仍標示池內（衝突標記 IpInPool）。
        let subnet = subnet("10.0.0.0/29", None, &[("10.0.0.6", "10.0.0.6")]);
        let assignments = [
            listed("10.0.0.1", "static", "資料庫主機", Some("eth0"), None),
            listed(
                "10.0.0.2",
                "reservation",
                "印表機",
                Some("wlan0"),
                Some("aa:bb:cc:dd:ee:ff"),
            ),
            listed("10.0.0.6", "static", "舊設備", Some("eth1"), None),
        ];

        let (items, total) = list(&subnet, &filter(None, 1, 50), &assignments).expect("推導成功");
        assert_eq!(total, 6);

        let entry = |address: &str| {
            items
                .iter()
                .find(|entry| entry.address == addr(address))
                .expect("位址存在")
        };

        let static_row = entry("10.0.0.1");
        assert_eq!(static_row.status, "static");
        assert_eq!(static_row.purpose, Some("static"));
        assert!(!static_row.in_pool);
        let target = static_row.assignment.as_ref().expect("含指派對象");
        assert_eq!(target.asset_description, "資料庫主機");
        assert_eq!(target.asset_location, "機房 A");
        assert_eq!(target.interface_name.as_deref(), Some("eth0"));
        assert_eq!(target.mac, None);

        let reservation_row = entry("10.0.0.2");
        assert_eq!(reservation_row.status, "reservation");
        assert_eq!(reservation_row.purpose, Some("reservation"));
        assert_eq!(
            reservation_row
                .assignment
                .as_ref()
                .expect("含指派對象")
                .mac
                .as_deref(),
            Some("aa:bb:cc:dd:ee:ff")
        );

        let in_pool_assigned = entry("10.0.0.6");
        assert!(in_pool_assigned.in_pool);
        assert_eq!(in_pool_assigned.status, "static", "已指派以用途為狀態");
        assert_eq!(
            in_pool_assigned.conflicts,
            vec!["IpInPool"],
            "指派落在 pool 內：標記衝突（見票 07）"
        );

        let available = entry("10.0.0.3");
        assert_eq!(available.status, "available");
        assert_eq!(available.purpose, None);
        assert!(available.assignment.is_none());
        assert!(available.conflicts.is_empty(), "無指派不標記衝突");
    }

    #[test]
    fn status_filter_limits_results() {
        let subnet = subnet("10.0.0.0/29", None, &[("10.0.0.6", "10.0.0.6")]);
        let assignments = [
            listed("10.0.0.1", "static", "資料庫主機", Some("eth0"), None),
            listed("10.0.0.2", "reservation", "印表機", Some("wlan0"), None),
        ];

        let (items, total) = list(
            &subnet,
            &status_filter(IpStatusFilter::Static),
            &assignments,
        )
        .expect("推導成功");
        assert_eq!(total, 1);
        assert_eq!(addresses(&items), ["10.0.0.1"]);

        let (items, total) = list(
            &subnet,
            &status_filter(IpStatusFilter::Reservation),
            &assignments,
        )
        .expect("推導成功");
        assert_eq!(total, 1);
        assert_eq!(addresses(&items), ["10.0.0.2"]);

        let (items, total) = list(
            &subnet,
            &status_filter(IpStatusFilter::InPool),
            &assignments,
        )
        .expect("推導成功");
        assert_eq!(total, 1);
        assert_eq!(addresses(&items), ["10.0.0.6"]);

        let (items, total) = list(
            &subnet,
            &status_filter(IpStatusFilter::Available),
            &assignments,
        )
        .expect("推導成功");
        assert_eq!(total, 3);
        assert_eq!(addresses(&items), ["10.0.0.3", "10.0.0.4", "10.0.0.5"]);
    }

    #[test]
    fn search_matches_assignment_fields() {
        let subnet = subnet("10.0.0.0/29", None, &[]);
        let assignments = [
            listed("10.0.0.1", "static", "資料庫主機", Some("eth0"), None),
            listed_at(
                "10.0.0.2",
                "reservation",
                "印表機",
                "Server Room B",
                Some("wlan0"),
                Some("AA:BB:CC:DD:EE:FF"),
            ),
        ];

        // 資產描述（不分大小寫）。
        let (items, total) =
            list(&subnet, &filter(Some("資料庫"), 1, 50), &assignments).expect("推導成功");
        assert_eq!(total, 1);
        assert_eq!(addresses(&items), ["10.0.0.1"]);

        // 介面名稱。
        let (items, total) =
            list(&subnet, &filter(Some("ETH0"), 1, 50), &assignments).expect("推導成功");
        assert_eq!(total, 1);
        assert_eq!(addresses(&items), ["10.0.0.1"]);

        // MAC 子字串（不分大小寫）。
        let (items, total) =
            list(&subnet, &filter(Some("bb:cc"), 1, 50), &assignments).expect("推導成功");
        assert_eq!(total, 1);
        assert_eq!(addresses(&items), ["10.0.0.2"]);

        // 資產位置（子字串、不分大小寫）。
        let (items, total) =
            list(&subnet, &filter(Some("機房"), 1, 50), &assignments).expect("推導成功");
        assert_eq!(total, 1);
        assert_eq!(addresses(&items), ["10.0.0.1"]);

        let (items, total) =
            list(&subnet, &filter(Some("SERVER ROOM"), 1, 50), &assignments).expect("推導成功");
        assert_eq!(total, 1);
        assert_eq!(addresses(&items), ["10.0.0.2"]);

        // 位址子字串與指派對象比對並存，維持數值排序與分頁。
        let (items, total) =
            list(&subnet, &filter(Some("10.0.0."), 2, 3), &assignments).expect("推導成功");
        assert_eq!(total, 6);
        assert_eq!(addresses(&items), ["10.0.0.4", "10.0.0.5", "10.0.0.6"]);

        // 無符合：空結果。
        let (items, total) =
            list(&subnet, &filter(Some("不存在"), 1, 50), &assignments).expect("推導成功");
        assert_eq!(total, 0);
        assert!(items.is_empty());
    }

    #[test]
    fn status_filter_parse_accepts_known_values_only() {
        assert_eq!(
            IpStatusFilter::parse("available"),
            Some(IpStatusFilter::Available)
        );
        assert_eq!(
            IpStatusFilter::parse("in_pool"),
            Some(IpStatusFilter::InPool)
        );
        assert_eq!(
            IpStatusFilter::parse("static"),
            Some(IpStatusFilter::Static)
        );
        assert_eq!(
            IpStatusFilter::parse("reservation"),
            Some(IpStatusFilter::Reservation)
        );
        assert_eq!(IpStatusFilter::parse("unknown"), None);
    }

    #[test]
    fn v6_list_returns_registered_addresses_in_numeric_order() {
        let subnet = subnet("fd00::/64", Some("fd00::1"), &[]);
        let assignments = [
            listed("fd00::10", "static", "主機十", Some("eth0"), None),
            listed("fd00::2", "static", "主機二", Some("eth1"), None),
            listed("fd00::1", "static", "閘道", Some("eth2"), None),
        ];

        let (items, total) = list(&subnet, &filter(None, 1, 50), &assignments).expect("推導成功");
        assert_eq!(total, 3, "僅列出已登錄位址");
        assert_eq!(
            addresses(&items),
            ["fd00::1", "fd00::2", "fd00::10"],
            "以數值（u128）升冪排序，非文字排序"
        );

        let first = &items[0];
        assert_eq!(first.status, "static");
        assert_eq!(first.purpose, Some("static"));
        assert!(!first.in_pool, "v6 無 pool 概念");
        assert!(first.is_gateway, "gateway 位址仍標記");
        assert_eq!(
            first
                .assignment
                .as_ref()
                .expect("含指派對象")
                .asset_description,
            "閘道"
        );
        assert!(first.conflicts.is_empty(), "網段內登錄位址無衝突");

        // 無登錄：空清單（不枚舉空閒位址）
        let (items, total) = list(&subnet, &filter(None, 1, 50), &[]).expect("推導成功");
        assert_eq!(total, 0);
        assert!(items.is_empty());
    }

    #[test]
    fn v6_list_supports_search_status_and_pagination() {
        let subnet = subnet("fd00::/64", None, &[]);
        let assignments = [
            listed("fd00::10", "static", "資料庫主機", Some("eth0"), None),
            listed_at(
                "fd00::2",
                "static",
                "印表機",
                "Server Room B",
                Some("wlan0"),
                Some("AA:BB:CC:DD:EE:FF"),
            ),
        ];

        // 伺服器端分頁（數值排序）
        let (items, total) = list(&subnet, &filter(None, 2, 1), &assignments).expect("推導成功");
        assert_eq!(total, 2);
        assert_eq!(addresses(&items), ["fd00::10"]);

        // 完整位址：不同壓縮寫法仍精確比對
        let (items, total) = list(
            &subnet,
            &filter(Some("fd00:0:0:0:0:0:0:10"), 1, 50),
            &assignments,
        )
        .expect("推導成功");
        assert_eq!(total, 1);
        assert_eq!(addresses(&items), ["fd00::10"]);

        // 關鍵字比對指派對象（資產描述／位置／介面名稱／MAC）；v4／v6 同一路徑
        for (query, address) in [
            ("資料庫", "fd00::10"),
            ("機房", "fd00::10"),
            ("ETH0", "fd00::10"),
            ("WLAN0", "fd00::2"),
            ("SERVER ROOM", "fd00::2"),
            ("bb:cc", "fd00::2"),
        ] {
            let (items, total) =
                list(&subnet, &filter(Some(query), 1, 50), &assignments).expect("推導成功");
            assert_eq!(total, 1, "q={query}");
            assert_eq!(addresses(&items), [address], "q={query}");
        }

        // 狀態篩選：static 全數；available／in_pool／reservation 皆空
        let (items, total) = list(
            &subnet,
            &status_filter(IpStatusFilter::Static),
            &assignments,
        )
        .expect("推導成功");
        assert_eq!(total, 2);
        assert_eq!(addresses(&items), ["fd00::2", "fd00::10"]);

        for status in [
            IpStatusFilter::Available,
            IpStatusFilter::InPool,
            IpStatusFilter::Reservation,
        ] {
            let (items, total) =
                list(&subnet, &status_filter(status), &assignments).expect("推導成功");
            assert_eq!(total, 0, "{status:?} 篩選應為空");
            assert!(items.is_empty());
        }
    }

    #[test]
    fn v6_list_keeps_out_of_subnet_registered_addresses() {
        // 網段縮小造成既有登錄位址出界：仍列出並標記 IpOutOfSubnet（見票 07）
        let subnet = subnet("fd00:0:0:1::/64", None, &[]);
        let assignments = [
            listed("fd00:0:0:1::5", "static", "主機", Some("eth0"), None),
            listed("fd00::5", "static", "出界主機", Some("eth1"), None),
        ];

        let (items, total) = list(&subnet, &filter(None, 1, 50), &assignments).expect("推導成功");
        assert_eq!(total, 2);
        assert_eq!(addresses(&items), ["fd00::5", "fd00:0:0:1::5"]);
        assert_eq!(
            items[0].conflicts,
            vec!["IpOutOfSubnet"],
            "出界登錄位址標記衝突"
        );
        assert!(items[1].conflicts.is_empty(), "網段內登錄位址無衝突");
    }

    #[test]
    fn v4_list_keeps_out_of_subnet_assignments_in_numeric_order() {
        // /25（126 個 host）聯集一筆 /24 時代的出界指派（.200）：總數 127。
        let subnet = subnet("10.0.0.0/25", None, &[]);
        let assignments = [listed(
            "10.0.0.200",
            "static",
            "出界主機",
            Some("eth0"),
            None,
        )];

        // 無條件：出界列排在數值順序位置（第 127 筆）
        let (items, total) = list(&subnet, &filter(None, 3, 50), &assignments).expect("推導成功");
        assert_eq!(total, 127);
        assert_eq!(items.len(), 27, "第 3 頁：host 101–126 與出界列");
        assert_eq!(items[26].address, addr("10.0.0.200"));
        assert_eq!(items[26].conflicts, vec!["IpOutOfSubnet"]);
        assert_eq!(items[26].status, "static");
        assert_eq!(
            items[26]
                .assignment
                .as_ref()
                .expect("含指派對象")
                .asset_description,
            "出界主機"
        );

        // 完整位址精確比對涵蓋出界列
        let (items, total) =
            list(&subnet, &filter(Some("10.0.0.200"), 1, 50), &assignments).expect("推導成功");
        assert_eq!(total, 1);
        assert_eq!(addresses(&items), ["10.0.0.200"]);

        // 狀態篩選涵蓋出界列
        let (items, total) = list(
            &subnet,
            &status_filter(IpStatusFilter::Static),
            &assignments,
        )
        .expect("推導成功");
        assert_eq!(total, 1);
        assert_eq!(addresses(&items), ["10.0.0.200"]);

        let (_, total) = list(
            &subnet,
            &status_filter(IpStatusFilter::Available),
            &assignments,
        )
        .expect("推導成功");
        assert_eq!(total, 126, "可用列仍為 host 數");

        // 關鍵字子字串比對涵蓋出界列
        let (items, total) =
            list(&subnet, &filter(Some("出界"), 1, 50), &assignments).expect("推導成功");
        assert_eq!(total, 1);
        assert_eq!(addresses(&items), ["10.0.0.200"]);
    }

    #[test]
    fn v4_list_marks_assignment_in_pool_and_out_of_subnet() {
        // 網段縮小且 pool 涵蓋出界位址：兩條規則並存（見 conflicts 固定順序）。
        let subnet = subnet("10.0.0.128/25", None, &[("10.0.0.130", "10.0.0.200")]);
        let assignments = [
            listed("10.0.0.5", "static", "出界主機", Some("eth0"), None),
            listed("10.0.0.150", "static", "池內主機", Some("eth1"), None),
        ];

        let (items, total) = list(&subnet, &filter(None, 1, 50), &assignments).expect("推導成功");
        assert_eq!(total, 127, "126 個 host 加 1 筆出界列");

        let entry = |address: &str| {
            items
                .iter()
                .find(|entry| entry.address == addr(address))
                .expect("位址存在")
        };

        assert_eq!(
            entry("10.0.0.5").conflicts,
            vec!["IpOutOfSubnet"],
            "出界且不在 pool 內"
        );
        assert_eq!(
            entry("10.0.0.150").conflicts,
            vec!["IpInPool"],
            "在 pool 內"
        );
    }

    #[test]
    fn v6_list_rejects_non_static_purpose_as_internal_error() {
        let subnet = subnet("fd00::/64", None, &[]);
        let assignments = [listed(
            "fd00::1",
            "reservation",
            "異常資料",
            Some("eth0"),
            Some("aa:bb:cc:dd:ee:ff"),
        )];

        assert!(
            list(&subnet, &filter(None, 1, 50), &assignments).is_err(),
            "v6 出現非 static 用途視為資料異常"
        );
    }
}
