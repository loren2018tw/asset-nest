//! 探測邊界：判定網段是否與本機同 L2（見 spec §探測邊界、ADR-0015）。
//!
//! 票 01 只實作本機判定 [`Prober::is_local`]；票 02 將擴充實際探測
//! （ARP 請求與回應收集）。實作以 `Arc<dyn Prober + Send + Sync>` 掛在
//! [`crate::AppState`]，測試注入 stub，任何碰真實網路者不進自動測試。

use std::net::Ipv4Addr;

use ipnet::Ipv4Net;

use crate::subnets::Subnet;

/// 探測邊界：觀測讀取端與掃描服務以注入的實作判定本機可觀測性。
pub trait Prober: Send + Sync {
    /// 本機是否有介面位址落在該 v4 子網（判定同 L2）。
    ///
    /// v6 網段與列舉失敗一律回 `false`（見 ADR-0015）。
    fn is_local(&self, subnet: &Subnet) -> bool;
}

/// 預設實作：以 Linux `getifaddrs` 列舉本機介面位址（見 ADR-0015）。
///
/// 不依賴 libpcap；非 Linux 建置一律回 `false`（列舉回空清單）。
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemProber;

impl SystemProber {
    pub fn new() -> Self {
        Self
    }
}

impl Prober for SystemProber {
    fn is_local(&self, subnet: &Subnet) -> bool {
        let Ok(network) = subnet.cidr.parse::<Ipv4Net>() else {
            return false; // v6 或非法 CIDR：一律非同 L2
        };
        contains_address(&network, &local_ipv4_addresses())
    }
}

/// 介面位址清單中是否有任一位址落在網段內（純函式，供單元測試）。
fn contains_address(network: &Ipv4Net, addresses: &[Ipv4Addr]) -> bool {
    addresses.iter().any(|address| network.contains(address))
}

/// 列舉本機介面的 IPv4 位址；列舉失敗回空清單。
#[cfg(target_os = "linux")]
fn local_ipv4_addresses() -> Vec<Ipv4Addr> {
    let mut addresses = Vec::new();

    // SAFETY: 依 getifaddrs(3) 契約——`list` 成功時由函式配置、
    // 由 freeifaddrs 釋放；走訪期間鏈結清單維持有效。
    unsafe {
        let mut list: *mut libc::ifaddrs = std::ptr::null_mut();
        if libc::getifaddrs(&mut list) != 0 {
            tracing::debug!("列舉本機網路介面失敗（getifaddrs），本機判定視為非同 L2");
            return addresses;
        }

        let mut current = list;
        while !current.is_null() {
            let interface = &*current;
            if !interface.ifa_addr.is_null()
                && (*interface.ifa_addr).sa_family as libc::c_int == libc::AF_INET
            {
                let address = &*(interface.ifa_addr as *const libc::sockaddr_in);
                addresses.push(Ipv4Addr::from(u32::from_be(address.sin_addr.s_addr)));
            }
            current = interface.ifa_next;
        }

        libc::freeifaddrs(list);
    }

    addresses
}

/// 非 Linux：不支援 `getifaddrs`，一律回空清單（見 ADR-0015）。
#[cfg(not(target_os = "linux"))]
fn local_ipv4_addresses() -> Vec<Ipv4Addr> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 測試用網段（欄位與 `subnets::tests::export_subnet` 一致）。
    fn subnet(cidr: &str) -> Subnet {
        Subnet {
            id: 0,
            cidr: cidr.to_string(),
            name: None,
            note: None,
            gateway: None,
            kea_subnet_id: None,
            pools: Vec::new(),
            observed: false,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    #[test]
    fn containment_matches_any_listed_address() {
        let network: Ipv4Net = "10.0.0.0/24".parse().expect("合法網段");
        let inside: Ipv4Addr = "10.0.0.123".parse().expect("合法位址");
        let outside: Ipv4Addr = "10.0.1.1".parse().expect("合法位址");

        assert!(!contains_address(&network, &[]), "無介面位址回 false");
        assert!(!contains_address(&network, &[outside]));
        assert!(contains_address(&network, &[outside, inside]));
        assert!(contains_address(&network, &[inside]));
    }

    #[test]
    fn v6_and_invalid_cidr_are_never_local() {
        let prober = SystemProber::new();
        assert!(!prober.is_local(&subnet("fd00::/64")), "v6 恆非同 L2");
        assert!(!prober.is_local(&subnet("not-a-cidr")));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn loopback_is_local_on_linux() {
        let prober = SystemProber::new();
        assert!(
            prober.is_local(&subnet("127.0.0.0/8")),
            "loopback 位址必落在本機介面清單"
        );
    }
}
