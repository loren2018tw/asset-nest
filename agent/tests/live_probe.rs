//! 真機唯讀：對本機所在 LAN 發少量 ARP 請求（僅送 ARP；不改任何設定）。
//!
//! 執行：`cargo test --manifest-path agent/Cargo.toml --test live_probe -- --ignored --nocapture`

#![cfg(target_os = "linux")]

use std::net::{IpAddr, Ipv4Addr};

use ipnet::Ipv4Net;

#[test]
#[ignore = "需要真機網路（與目標網段同 L2）與 CAP_NET_RAW"]
fn raw_probe_live_lan_read_only() {
    // 以 UDP connect 取得對外路由所用的本機位址；不會送出任何封包。
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").expect("綁定 UDP socket");
    socket
        .connect("1.1.1.1:9")
        .expect("建立路由查詢（connect 不送封包）");
    let local = match socket.local_addr().expect("讀取本機位址").ip() {
        IpAddr::V4(address) if !address.is_loopback() => address,
        other => panic!("找不到對外 IPv4 介面（{other}）；本測試需與目標網段同 L2 的網路"),
    };

    // 無 netmask 可讀時以 /24 近似；僅影響目標集合。
    let network = Ipv4Net::new(local, 24).expect("合法 /24").trunc();

    // 目標：預設閘道（由 /proc/net/route 讀取）；讀不到則用第一個 host。
    let gateway = default_gateway();
    let mut targets: Vec<Ipv4Addr> = gateway.into_iter().chain(network.hosts().take(1)).collect();
    targets.sort_unstable();
    targets.dedup();

    let result = asset_nest_agent::probe::probe_targets(&network, &targets, 1_000)
        .unwrap_or_else(|error| panic!("raw 探測失敗（需 CAP_NET_RAW）：{error}"));

    println!(
        "本機 {local}、網段 {network}、目標 {targets:?} → ARP 回應 {:?}",
        result.seen
    );
    assert_eq!(result.checked, targets, "checked 為全部送出的目標");
    for (address, mac) in &result.seen {
        assert!(
            targets.contains(address),
            "回應位址須在目標集合內：{address}"
        );
        assert!(is_mac_like(mac), "MAC 格式不合理：{mac}");
    }
    if result.seen.is_empty() {
        println!("（本次無 ARP 回應；可能目標裝置不存在）");
    }
}

/// 由 `/proc/net/route` 讀取預設閘道（無預設路由回 `None`）。
fn default_gateway() -> Option<Ipv4Addr> {
    let table = std::fs::read_to_string("/proc/net/route").ok()?;
    for line in table.lines().skip(1) {
        let words: Vec<&str> = line.split_whitespace().collect();
        if words.len() < 3 || words[1] != "00000000" {
            continue;
        }
        let raw = u32::from_str_radix(words[2], 16).ok()?;
        let address = Ipv4Addr::from(raw.swap_bytes());
        if !address.is_unspecified() {
            return Some(address);
        }
    }
    None
}

/// 寬鬆 MAC 檢查：`:` 分隔的 6 組兩位十六進位（小寫正規格式）。
fn is_mac_like(value: &str) -> bool {
    let groups: Vec<&str> = value.split(':').collect();
    groups.len() == 6
        && groups.iter().all(|group| {
            group.len() == 2
                && group
                    .chars()
                    .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        })
}
