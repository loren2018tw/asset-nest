//! 連線主機 MAC 反查：由連線來源 IP 讀取 `/proc/net/arp`（比照 Kealight 做法）。
//!
//! 瀏覽器拿不到操作主機的 MAC；操作主機與本系統在同一層網路（同廣播域）時，
//! 伺服器可從自身 ARP 表反查連線來源 IP。跨網段、VPN、IPv6 或非 Linux
//! 一律回 `None`，不做本機網卡 fallback。

use std::net::IpAddr;

/// 解析 `/proc/net/arp` 文字，反查 `target_ip` 對應的 MAC。
///
/// 標題列、incomplete（`00:00:00:00:00:00`）或無命中皆回 `None`；
/// 讀不到 ARP 表（Windows／macOS）由 [`peer_mac`] 回 `None`。
pub fn mac_from_arp_table(table: &str, target_ip: &str) -> Option<String> {
    for line in table.lines() {
        let words: Vec<&str> = line.split_whitespace().collect();
        if words.len() < 4 || words[0] == "IP" {
            continue;
        }
        if words[0] == target_ip && words[3] != "00:00:00:00:00:00" {
            return Some(words[3].to_string());
        }
    }
    None
}

/// 讀取本機 ARP 表反查 `ip` 的 MAC；IPv6 或讀不到 ARP 表時回 `None`。
pub fn peer_mac(ip: IpAddr) -> Option<String> {
    match ip {
        IpAddr::V4(ip) => {
            let table = std::fs::read_to_string("/proc/net/arp").ok()?;
            mac_from_arp_table(&table, &ip.to_string())
        }
        IpAddr::V6(_) => None,
    }
}

/// 決定反查 ARP 用的來源 IP。
///
/// 開發時 Quasar dev server 以 Vite proxy 轉送請求（帶 `X-Forwarded-For`），
/// 直接連線來源為 loopback；此時取 XFF 第一段（原始客戶端）。非 loopback
/// 一律用連線來源 IP，忽略 XFF 以免遠端偽造；XFF 無法解析時亦退回連線來源。
pub fn resolve_peer_ip(remote: IpAddr, forwarded_for: Option<&str>) -> IpAddr {
    if !remote.is_loopback() {
        return remote;
    }
    forwarded_for
        .and_then(|value| value.split(',').next())
        .and_then(|first| first.trim().parse::<IpAddr>().ok())
        .unwrap_or(remote)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 比照 `/proc/net/arp` 實際格式：標題列＋一筆命中＋一筆 incomplete。
    const ARP_TABLE: &str = "\
IP address       HW type     Flags       HW address            Mask     Device\n\
10.1.1.2         0x1         0x2         aa:bb:cc:dd:ee:ff     *        eth0\n\
10.1.1.3         0x1         0x0         00:00:00:00:00:00     *        eth0";

    #[test]
    fn arp_lookup_hits_and_misses() {
        assert_eq!(
            mac_from_arp_table(ARP_TABLE, "10.1.1.2"),
            Some("aa:bb:cc:dd:ee:ff".to_string())
        );
        assert!(
            mac_from_arp_table(ARP_TABLE, "10.1.1.3").is_none(),
            "incomplete（00:00:00:00:00:00）應視為查無"
        );
        assert!(
            mac_from_arp_table(ARP_TABLE, "10.9.9.9").is_none(),
            "無命中的 IP 應回 None"
        );
    }

    #[test]
    fn arp_lookup_ignores_header_and_short_lines() {
        // 若未跳過標題列，words[0] == "IP" 且 words[3] == "type" 會被誤判為命中
        let header =
            "IP address       HW type     Flags       HW address            Mask     Device\n";
        assert!(mac_from_arp_table(header, "IP").is_none());
        assert!(mac_from_arp_table("10.1.1.2 0x1 0x2\n", "10.1.1.2").is_none());
    }

    #[test]
    fn forwarded_for_is_used_only_from_loopback() {
        let loopback: IpAddr = "127.0.0.1".parse().expect("127.0.0.1");
        let forwarded: IpAddr = "203.0.113.7".parse().expect("203.0.113.7");

        // loopback＋XFF：採原始客戶端（開發時 Vite proxy）
        assert_eq!(resolve_peer_ip(loopback, Some("203.0.113.7")), forwarded);
        // ::1 亦視為 loopback
        let loopback_v6: IpAddr = "::1".parse().expect("::1");
        assert_eq!(resolve_peer_ip(loopback_v6, Some("203.0.113.7")), forwarded);
        // loopback 無 XFF：用連線來源
        assert_eq!(resolve_peer_ip(loopback, None), loopback);

        // 非 loopback 帶 XFF：忽略偽造標頭，仍用連線來源
        let remote: IpAddr = "198.51.100.9".parse().expect("198.51.100.9");
        assert_eq!(resolve_peer_ip(remote, Some("203.0.113.7")), remote);
    }

    #[test]
    fn forwarded_for_takes_first_segment_and_falls_back_on_invalid() {
        let loopback: IpAddr = "127.0.0.1".parse().expect("127.0.0.1");
        let forwarded: IpAddr = "203.0.113.7".parse().expect("203.0.113.7");

        assert_eq!(
            resolve_peer_ip(loopback, Some("203.0.113.7, 10.0.0.1")),
            forwarded,
            "多段 XFF 取第一段"
        );
        assert_eq!(
            resolve_peer_ip(loopback, Some("not-an-ip")),
            loopback,
            "XFF 無法解析時退回連線來源"
        );
        assert_eq!(
            resolve_peer_ip(loopback, Some("  203.0.113.7  ")),
            forwarded,
            "XFF 前後空白應容忍"
        );
    }
}
