//! 位址空間（Address Space）：位址落點判定的單一 module（見
//! `.scratch/address-space/spec.md`）。
//!
//! 把「這個位址落在網段裡的哪裡？」收成一處：CIDR 解析與容納（v4／v6）、
//! v4 host 範圍、pool／排除範圍與單一位址落點。純資料結構：不快取、不進
//! `AppState`、不碰 DB；呼叫端在需要處建一次、以 `&AddressSpace` 向下傳。
//! 落點優先序固定為 `Outside > NonHost > InPool > InExcluded > Host`；
//! v6 只有 `Outside`／`Host`（見 spec §介面）。

use std::fmt;
use std::net::{AddrParseError, IpAddr, Ipv4Addr};

use ipnet::{IpNet, Ipv4Net};

use crate::api::ApiError;
use crate::subnets::{Exclusion, Pool, Subnet};

/// 由 [`Subnet`] 解析出的位址空間：網段、host 範圍、pool 與排除範圍。
///
/// 純資料結構；不快取、不進 `AppState`、不碰 DB。pool 與排除範圍為 v4 限定
/// （v6 網段忽略；寫入端已擋），且保持輸入順序。
#[derive(Debug)]
pub struct AddressSpace {
    network: IpNet,
    /// v4 host 範圍；v6 為 `None`。
    hosts: Option<Ipv4Range>,
    pools: Vec<Ipv4Range>,
    exclusions: Vec<Ipv4Range>,
}

impl AddressSpace {
    /// 由網段解析位址空間；CIDR、host 範圍、pool 與排除範圍各解析一次。
    ///
    /// 只驗證可解析與「起點不大於終點」；不重驗 pool⊆CIDR、排除範圍×pool
    /// 重疊等結構規則（寫入端職責；見 ADR-0006、ADR-0020）。失敗一律視為
    /// 資料損壞，回 [`AddressSpaceError`]。
    pub fn new(subnet: &Subnet) -> Result<Self, AddressSpaceError> {
        let network: IpNet = subnet
            .cidr
            .parse()
            .map_err(AddressSpaceError::InvalidCidr)?;
        let IpNet::V4(v4_network) = network else {
            // v6：pool／排除範圍為 v4 限定，忽略（寫入端已擋；見 spec §決策 5）。
            return Ok(Self {
                network,
                hosts: None,
                pools: Vec::new(),
                exclusions: Vec::new(),
            });
        };

        Ok(Self {
            network,
            hosts: Some(host_range(&v4_network)),
            pools: parse_pools(&subnet.pools)?,
            exclusions: parse_exclusions(&subnet.exclusions)?,
        })
    }

    /// 單一位址的落點；總函式、不失敗。
    ///
    /// v4 依優先序 `Outside > NonHost > InPool > InExcluded > Host`；v6 只有
    /// `Outside`／`Host`。地址族不符（如 v6 位址對 v4 網段）視為 `Outside`。
    pub fn placement(&self, address: IpAddr) -> Placement {
        match (self.network, address) {
            (IpNet::V4(network), IpAddr::V4(address)) => {
                if !network.contains(&address) {
                    return Placement::Outside;
                }
                if !self.hosts.is_some_and(|hosts| hosts.contains(address)) {
                    return Placement::NonHost;
                }
                if self.pools.iter().any(|pool| pool.contains(address)) {
                    return Placement::InPool;
                }
                if self
                    .exclusions
                    .iter()
                    .any(|exclusion| exclusion.contains(address))
                {
                    return Placement::InExcluded;
                }
                Placement::Host
            }
            (IpNet::V6(network), IpAddr::V6(address)) => {
                if network.contains(&address) {
                    Placement::Host
                } else {
                    Placement::Outside
                }
            }
            _ => Placement::Outside,
        }
    }

    /// 網段 CIDR。
    pub fn network(&self) -> IpNet {
        self.network
    }

    /// v4 host 範圍（端點皆含）；v6 回 `None`。
    pub fn hosts(&self) -> Option<Ipv4Range> {
        self.hosts
    }

    /// pool 範圍（端點皆含）；保持輸入順序。
    pub fn pools(&self) -> &[Ipv4Range] {
        &self.pools
    }

    /// 排除範圍（端點皆含）；保持輸入順序。
    pub fn exclusions(&self) -> &[Ipv4Range] {
        &self.exclusions
    }
}

/// 單一位址相對網段的落點；判斷優先序固定（見 spec §介面）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    /// 網段外（含地址族不符）。
    Outside,
    /// 網段內但非 host（network／broadcast）。
    NonHost,
    /// DHCP 位址池內。
    InPool,
    /// 排除範圍內。
    InExcluded,
    /// 可用 host 位址。
    Host,
}

/// v4 位址範圍；以 u32 為底、端點皆含。
///
/// 建構子不公開：範圍一律由 [`AddressSpace`] 的解析路徑建立（起點不大於終點）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Ipv4Range {
    start: u32,
    end: u32,
}

impl Ipv4Range {
    /// 以 u32 端點建構；僅 module 內部使用，呼叫端均已確保 `start <= end`。
    fn from_bits(start: u32, end: u32) -> Self {
        Self { start, end }
    }

    /// 由端點建構；起點大於終點回傳 `None`（顛倒視為資料損壞）。
    fn from_endpoints(start: Ipv4Addr, end: Ipv4Addr) -> Option<Self> {
        (start <= end).then(|| Self::from_bits(start.to_bits(), end.to_bits()))
    }

    /// 範圍起點。
    pub fn start(&self) -> Ipv4Addr {
        Ipv4Addr::from(self.start)
    }

    /// 範圍終點。
    pub fn end(&self) -> Ipv4Addr {
        Ipv4Addr::from(self.end)
    }

    /// 位址是否在範圍內（端點皆含）。
    pub fn contains(&self, address: Ipv4Addr) -> bool {
        (self.start..=self.end).contains(&address.to_bits())
    }

    /// 範圍位址總數；以 u64 計算避免溢位。
    pub fn count(&self) -> u64 {
        u64::from(self.end) - u64::from(self.start) + 1
    }

    /// 數值排序第 `offset` 個位址（0 起算）；超出範圍回傳 `None`。
    pub fn nth(&self, offset: u64) -> Option<Ipv4Addr> {
        let value = u64::from(self.start).checked_add(offset)?;
        (value <= u64::from(self.end)).then(|| Ipv4Addr::from(value as u32))
    }

    /// 依數值升冪走訪範圍內所有位址（端點皆含）。
    pub fn iter(&self) -> impl Iterator<Item = Ipv4Addr> + '_ {
        (u64::from(self.start)..=u64::from(self.end)).map(|value| Ipv4Addr::from(value as u32))
    }
}

/// 建構 [`AddressSpace`] 的錯誤；資料損壞（寫入驗證不可達；見 ADR-0006、ADR-0020）。
#[derive(Debug)]
pub enum AddressSpaceError {
    /// 網段 CIDR 無法解析。
    InvalidCidr(ipnet::AddrParseError),
    /// pool 起點或終點無法解析。
    InvalidPool(AddrParseError),
    /// 排除範圍起點或終點無法解析。
    InvalidExclusion(AddrParseError),
    /// 範圍起點大於終點；保留資料庫原文，供訊息與計畫輸出逐字呈現。
    ReversedPool { start: String, end: String },
}

impl fmt::Display for AddressSpaceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidCidr(source) => write!(f, "網段 CIDR 格式錯誤：{source}"),
            Self::InvalidPool(source) => write!(f, "pool 位址格式錯誤：{source}"),
            Self::InvalidExclusion(source) => write!(f, "排除範圍位址格式錯誤：{source}"),
            Self::ReversedPool { start, end } => write!(f, "pool 範圍顛倒：{start}-{end}"),
        }
    }
}

impl From<AddressSpaceError> for ApiError {
    /// 資料損壞一律視為內部錯誤（500）；`context` 為對應句首（見 spec §錯誤）。
    fn from(error: AddressSpaceError) -> Self {
        let context = match &error {
            AddressSpaceError::InvalidCidr(_) => "網段 CIDR 格式錯誤",
            AddressSpaceError::InvalidPool(_) => "pool 位址格式錯誤",
            AddressSpaceError::InvalidExclusion(_) => "排除範圍位址格式錯誤",
            AddressSpaceError::ReversedPool { .. } => "pool 範圍顛倒",
        };
        ApiError::internal(context, &error)
    }
}

/// 由 v4 CIDR 推導 host 範圍；端點皆含。
///
/// `/31`（RFC 3021 點對點）與 `/32` 全數列出；其餘扣除 network 與 broadcast
/// （沿用 `ips::HostRange::of` 語意；見 spec §介面）。
fn host_range(network: &Ipv4Net) -> Ipv4Range {
    let base = u64::from(u32::from(network.network()));
    let size = 1u64 << (32 - network.prefix_len());
    match network.prefix_len() {
        31 | 32 => Ipv4Range::from_bits(base as u32, (base + size - 1) as u32),
        _ => Ipv4Range::from_bits((base + 1) as u32, (base + size - 2) as u32),
    }
}

/// 解析 pool 範圍為 v4 範圍；格式異常回 [`AddressSpaceError::InvalidPool`]
/// （資料庫內容經結構驗證，異常視為資料損壞）。
fn parse_pools(pools: &[Pool]) -> Result<Vec<Ipv4Range>, AddressSpaceError> {
    pools
        .iter()
        .map(|pool| {
            let start: Ipv4Addr = pool
                .start_ip
                .parse()
                .map_err(AddressSpaceError::InvalidPool)?;
            let end: Ipv4Addr = pool
                .end_ip
                .parse()
                .map_err(AddressSpaceError::InvalidPool)?;
            Ipv4Range::from_endpoints(start, end).ok_or_else(|| AddressSpaceError::ReversedPool {
                start: pool.start_ip.clone(),
                end: pool.end_ip.clone(),
            })
        })
        .collect()
}

/// 解析排除範圍為 v4 範圍；格式異常回 [`AddressSpaceError::InvalidExclusion`]。
///
/// 起點大於終點為資料損壞（寫入端已擋；見 ADR-0020），以唯一的顛倒變體
/// [`AddressSpaceError::ReversedPool`] 表示。
fn parse_exclusions(exclusions: &[Exclusion]) -> Result<Vec<Ipv4Range>, AddressSpaceError> {
    exclusions
        .iter()
        .map(|exclusion| {
            let start: Ipv4Addr = exclusion
                .start_ip
                .parse()
                .map_err(AddressSpaceError::InvalidExclusion)?;
            let end: Ipv4Addr = exclusion
                .end_ip
                .parse()
                .map_err(AddressSpaceError::InvalidExclusion)?;
            Ipv4Range::from_endpoints(start, end).ok_or_else(|| AddressSpaceError::ReversedPool {
                start: exclusion.start_ip.clone(),
                end: exclusion.end_ip.clone(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::subnets::{Exclusion, Pool};

    /// 測試用 v4 位址。
    fn addr(text: &str) -> Ipv4Addr {
        text.parse().expect("合法位址")
    }

    /// 測試用 v4 位址（`IpAddr` 形態）。
    fn v4(text: &str) -> IpAddr {
        IpAddr::V4(addr(text))
    }

    /// 測試用 v6 位址（`IpAddr` 形態）。
    fn v6(text: &str) -> IpAddr {
        text.parse().expect("合法位址")
    }

    /// 測試用網段；pools 以（起點、終點）表示（比照 `conflicts` 測試 helper）。
    fn subnet(cidr: &str, pools: &[(&str, &str)]) -> Subnet {
        Subnet {
            id: 1,
            cidr: cidr.to_string(),
            name: None,
            note: None,
            gateway: None,
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
            exclusions: vec![],
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    /// 測試用網段：附排除範圍（僅 v4；見 ADR-0020）。
    fn subnet_with_exclusions(
        cidr: &str,
        pools: &[(&str, &str)],
        exclusions: &[(&str, &str)],
    ) -> Subnet {
        let mut subnet = subnet(cidr, pools);
        subnet.exclusions = exclusions
            .iter()
            .enumerate()
            .map(|(index, (start, end))| Exclusion {
                id: index as i64 + 1,
                start_ip: start.to_string(),
                end_ip: end.to_string(),
                note: None,
            })
            .collect();
        subnet
    }

    /// 測試用位址空間。
    fn space_of(subnet: &Subnet) -> AddressSpace {
        AddressSpace::new(subnet).expect("位址空間建構成功")
    }

    #[test]
    fn placement_follows_priority_order() {
        // pool .2–.3、排除範圍 .4–.5；network .0、broadcast .7。
        let space = space_of(&subnet_with_exclusions(
            "10.0.0.0/29",
            &[("10.0.0.2", "10.0.0.3")],
            &[("10.0.0.4", "10.0.0.5")],
        ));
        assert_eq!(
            space.network(),
            "10.0.0.0/29".parse::<IpNet>().expect("合法 CIDR"),
            "網段 CIDR 原樣保留"
        );

        assert_eq!(
            space.placement(v4("10.0.9.9")),
            Placement::Outside,
            "網段外"
        );
        assert_eq!(
            space.placement(v4("10.0.0.0")),
            Placement::NonHost,
            "network"
        );
        assert_eq!(
            space.placement(v4("10.0.0.7")),
            Placement::NonHost,
            "broadcast"
        );
        assert_eq!(space.placement(v4("10.0.0.1")), Placement::Host);
        assert_eq!(space.placement(v4("10.0.0.2")), Placement::InPool);
        assert_eq!(space.placement(v4("10.0.0.3")), Placement::InPool);
        assert_eq!(space.placement(v4("10.0.0.4")), Placement::InExcluded);
        assert_eq!(space.placement(v4("10.0.0.5")), Placement::InExcluded);
        assert_eq!(space.placement(v4("10.0.0.6")), Placement::Host);
    }

    #[test]
    fn placement_prefers_outside_and_non_host_on_corrupt_data() {
        // 寫入端已擋 pool⊆CIDR 與排除範圍⊆CIDR（見 ADR-0006、ADR-0020）；
        // 防禦性：Outside／NonHost 仍優先於 InPool／InExcluded。
        let space = space_of(&subnet_with_exclusions(
            "10.0.0.0/29",
            &[("10.0.0.0", "10.0.0.2")],
            &[("10.0.9.9", "10.0.9.9")],
        ));

        assert_eq!(
            space.placement(v4("10.0.0.0")),
            Placement::NonHost,
            "network 先於 pool"
        );
        assert_eq!(space.placement(v4("10.0.0.1")), Placement::InPool);
        assert_eq!(
            space.placement(v4("10.0.9.9")),
            Placement::Outside,
            "網段外先於排除範圍"
        );
    }

    #[test]
    fn pool_outranks_exclusion_when_overlapping() {
        // 寫入端已擋排除範圍×pool 重疊（見 ADR-0020）；防禦性：同時命中時 pool 先。
        let space = space_of(&subnet_with_exclusions(
            "10.0.0.0/29",
            &[("10.0.0.2", "10.0.0.3")],
            &[("10.0.0.2", "10.0.0.5")],
        ));

        assert_eq!(space.placement(v4("10.0.0.2")), Placement::InPool);
        assert_eq!(space.placement(v4("10.0.0.3")), Placement::InPool);
        assert_eq!(space.placement(v4("10.0.0.4")), Placement::InExcluded);
        assert_eq!(space.placement(v4("10.0.0.5")), Placement::InExcluded);
    }

    #[test]
    fn pool_and_exclusion_endpoints_are_inclusive() {
        let space = space_of(&subnet_with_exclusions(
            "10.0.0.0/24",
            &[("10.0.0.10", "10.0.0.20")],
            &[("10.0.0.30", "10.0.0.31")],
        ));

        assert_eq!(
            space.placement(v4("10.0.0.10")),
            Placement::InPool,
            "pool 起點含"
        );
        assert_eq!(
            space.placement(v4("10.0.0.20")),
            Placement::InPool,
            "pool 終點含"
        );
        assert_eq!(space.placement(v4("10.0.0.9")), Placement::Host);
        assert_eq!(space.placement(v4("10.0.0.21")), Placement::Host);
        assert_eq!(
            space.placement(v4("10.0.0.30")),
            Placement::InExcluded,
            "排除範圍起點含"
        );
        assert_eq!(
            space.placement(v4("10.0.0.31")),
            Placement::InExcluded,
            "排除範圍終點含"
        );
        assert_eq!(space.placement(v4("10.0.0.32")), Placement::Host);

        // 範圍端點與容納判定（module interface）。
        let pool = space.pools().first().expect("一個 pool");
        assert_eq!(pool.start(), addr("10.0.0.10"));
        assert_eq!(pool.end(), addr("10.0.0.20"));
        assert!(pool.contains(addr("10.0.0.10")), "起點含");
        assert!(pool.contains(addr("10.0.0.20")), "終點含");
        let exclusion = space.exclusions().first().expect("一個排除範圍");
        assert_eq!(exclusion.start(), addr("10.0.0.30"));
        assert_eq!(exclusion.end(), addr("10.0.0.31"));
    }

    #[test]
    fn network_and_broadcast_are_non_host() {
        let space = space_of(&subnet("10.0.0.0/24", &[]));

        assert_eq!(space.placement(v4("10.0.0.0")), Placement::NonHost);
        assert_eq!(space.placement(v4("10.0.0.255")), Placement::NonHost);
        assert_eq!(space.placement(v4("10.0.0.1")), Placement::Host);
        assert_eq!(space.placement(v4("10.0.0.254")), Placement::Host);
    }

    #[test]
    fn hosts_list_all_for_slash31_and_slash32() {
        let space = space_of(&subnet("10.0.1.0/31", &[]));
        let hosts = space.hosts().expect("v4 有 host 範圍");
        assert_eq!(hosts.count(), 2, "/31 全數列出");
        assert_eq!(hosts.nth(0), Some(addr("10.0.1.0")));
        assert_eq!(hosts.nth(1), Some(addr("10.0.1.1")));
        assert_eq!(hosts.nth(2), None);
        assert_eq!(
            space.placement(v4("10.0.1.0")),
            Placement::Host,
            "/31 網路地址亦為 host"
        );

        let space = space_of(&subnet("10.0.2.7/32", &[]));
        let hosts = space.hosts().expect("v4 有 host 範圍");
        assert_eq!(hosts.count(), 1, "/32 全數列出");
        assert_eq!(hosts.nth(0), Some(addr("10.0.2.7")));
        assert_eq!(hosts.nth(1), None);
        assert_eq!(space.placement(v4("10.0.2.7")), Placement::Host);
        assert_eq!(space.placement(v4("10.0.2.8")), Placement::Outside);
    }

    #[test]
    fn hosts_support_any_prefix_and_count_zero_prefix() {
        let space = space_of(&subnet("10.0.0.0/30", &[]));
        let hosts = space.hosts().expect("v4 有 host 範圍");
        assert_eq!(hosts.count(), 2);
        assert_eq!(hosts.nth(0), Some(addr("10.0.0.1")));
        assert_eq!(hosts.nth(1), Some(addr("10.0.0.2")));

        let space = space_of(&subnet("0.0.0.0/0", &[]));
        let hosts = space.hosts().expect("v4 有 host 範圍");
        assert_eq!(
            hosts.count(),
            u64::from(u32::MAX) - 1,
            "0.0.0.0/0 扣 network/broadcast"
        );
        assert_eq!(hosts.nth(0), Some(addr("0.0.0.1")));
        assert_eq!(hosts.nth(hosts.count() - 1), Some(addr("255.255.255.254")));
    }

    #[test]
    fn hosts_nth_and_iter_bounds_are_consistent() {
        let space = space_of(&subnet("10.0.0.0/30", &[]));
        let hosts = space.hosts().expect("v4 有 host 範圍");

        assert_eq!(hosts.nth(0), Some(addr("10.0.0.1")), "首端");
        assert_eq!(hosts.nth(1), Some(addr("10.0.0.2")), "末端");
        assert_eq!(hosts.nth(2), None, "超出回 None");
        assert_eq!(hosts.nth(u64::MAX), None, "極大位移不溢位");

        let items: Vec<Ipv4Addr> = hosts.iter().collect();
        assert_eq!(items, vec![addr("10.0.0.1"), addr("10.0.0.2")]);
        assert_eq!(
            hosts.iter().count() as u64,
            hosts.count(),
            "iter 與 count 一致"
        );
        assert_eq!(hosts.iter().next(), hosts.nth(0), "iter 起點與 nth 一致");
        assert_eq!(
            hosts.iter().last(),
            hosts.nth(hosts.count() - 1),
            "iter 訖點與 nth 一致"
        );
    }

    #[test]
    fn placement_v6_is_host_inside_and_outside_otherwise() {
        // 寫入端禁 v6 pool／排除範圍；此處防禦性帶入，module 應忽略（見 spec §決策 5）。
        let space = space_of(&subnet_with_exclusions(
            "fd00:0:0:1::/64",
            &[("fd00:0:0:1::5", "fd00:0:0:1::9")],
            &[("fd00:0:0:1::a", "fd00:0:0:1::f")],
        ));

        assert_eq!(
            space.placement(v6("fd00:0:0:1::1")),
            Placement::Host,
            "CIDR 內"
        );
        assert_eq!(
            space.placement(v6("fd00:0:0:2::1")),
            Placement::Outside,
            "CIDR 外"
        );
        assert!(space.hosts().is_none(), "v6 無 host 範圍");
        assert!(space.pools().is_empty(), "v6 忽略 pool");
        assert!(space.exclusions().is_empty(), "v6 忽略排除範圍");

        assert_eq!(
            space.placement(v4("10.0.0.1")),
            Placement::Outside,
            "地址族不符"
        );
        let v4_space = space_of(&subnet("10.0.0.0/24", &[]));
        assert_eq!(
            v4_space.placement(v6("fd00::1")),
            Placement::Outside,
            "v4 網段配 v6 位址"
        );
    }

    #[test]
    fn invalid_cidr_is_rejected() {
        let error = AddressSpace::new(&subnet("not-a-cidr", &[])).expect_err("非法 CIDR");
        assert!(matches!(error, AddressSpaceError::InvalidCidr(_)));
        assert!(
            error.to_string().starts_with("網段 CIDR 格式錯誤："),
            "文案：{error}"
        );
    }

    #[test]
    fn invalid_pool_address_is_rejected() {
        let error = AddressSpace::new(&subnet("10.0.0.0/24", &[("nope", "10.0.0.9")]))
            .expect_err("非法 pool 位址");
        assert!(matches!(error, AddressSpaceError::InvalidPool(_)));
        assert!(
            error.to_string().starts_with("pool 位址格式錯誤："),
            "文案：{error}"
        );
    }

    #[test]
    fn invalid_exclusion_address_is_rejected() {
        let error = AddressSpace::new(&subnet_with_exclusions(
            "10.0.0.0/24",
            &[],
            &[("10.0.0.9", "nope")],
        ))
        .expect_err("非法排除範圍位址");
        assert!(matches!(error, AddressSpaceError::InvalidExclusion(_)));
        assert!(
            error.to_string().starts_with("排除範圍位址格式錯誤："),
            "文案：{error}"
        );
    }

    #[test]
    fn reversed_pool_is_rejected_with_stored_text() {
        let error = AddressSpace::new(&subnet("10.0.0.0/24", &[("10.0.0.9", "10.0.0.1")]))
            .expect_err("起點大於終點");
        match &error {
            AddressSpaceError::ReversedPool { start, end } => {
                assert_eq!(start, "10.0.0.9", "保留資料庫原文");
                assert_eq!(end, "10.0.0.1", "保留資料庫原文");
            }
            other => panic!("應為 ReversedPool：{other:?}"),
        }
        assert_eq!(error.to_string(), "pool 範圍顛倒：10.0.0.9-10.0.0.1");
    }

    #[test]
    fn reversed_exclusion_is_rejected() {
        // 寫入端已擋（見 ADR-0020）；顛倒排除範圍為資料損壞，以唯一的顛倒
        // 變體 ReversedPool 表示。
        let error = AddressSpace::new(&subnet_with_exclusions(
            "10.0.0.0/24",
            &[],
            &[("10.0.0.9", "10.0.0.1")],
        ))
        .expect_err("起點大於終點");
        assert!(matches!(error, AddressSpaceError::ReversedPool { .. }));
    }

    #[test]
    fn errors_convert_to_internal_api_error_with_sentence_context() {
        let invalid_cidr = AddressSpace::new(&subnet("nope", &[])).expect_err("非法 CIDR");
        assert_eq!(ApiError::from(invalid_cidr).message(), "網段 CIDR 格式錯誤");

        let invalid_pool = AddressSpace::new(&subnet("10.0.0.0/24", &[("nope", "10.0.0.9")]))
            .expect_err("非法 pool 位址");
        assert_eq!(ApiError::from(invalid_pool).message(), "pool 位址格式錯誤");

        let invalid_exclusion = AddressSpace::new(&subnet_with_exclusions(
            "10.0.0.0/24",
            &[],
            &[("10.0.0.9", "nope")],
        ))
        .expect_err("非法排除範圍位址");
        assert_eq!(
            ApiError::from(invalid_exclusion).message(),
            "排除範圍位址格式錯誤"
        );

        let reversed = AddressSpace::new(&subnet("10.0.0.0/24", &[("10.0.0.9", "10.0.0.1")]))
            .expect_err("起點大於終點");
        assert_eq!(ApiError::from(reversed).message(), "pool 範圍顛倒");
    }
}
