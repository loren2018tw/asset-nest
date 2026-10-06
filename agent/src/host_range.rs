//! v4 網段的 host 位址範圍（自 backend `ips::HostRange` 複製；語意見 spec
//! §週期掃描）：扣除 network／broadcast；`/31`、`/32` 全數列出。

use std::net::Ipv4Addr;

use ipnet::Ipv4Net;

/// v4 網段的 host 位址範圍；端點皆含。
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 測試用位址。
    fn addr(text: &str) -> Ipv4Addr {
        text.parse().expect("合法位址")
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
}
