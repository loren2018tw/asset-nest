//! 真機唯讀：短窗被動監聽本機所在 LAN 的 ARP（只收不送；不改任何設定），
//! 並驗證被動監聽與主動探測可同時執行（互不阻塞）。
//!
//! 執行：`cargo test --manifest-path agent/Cargo.toml --test live_passive -- --ignored --nocapture`

#![cfg(target_os = "linux")]

use std::net::{IpAddr, Ipv4Addr};
use std::time::Duration;

use ipnet::Ipv4Net;

#[test]
#[ignore = "需要真機網路（與目標網段同 L2）與 CAP_NET_RAW"]
fn passive_observe_live_lan_read_only() {
    let (local, network) = local_network();

    let senders = asset_nest_agent::probe::passive_observe(&network, Duration::from_secs(5))
        .unwrap_or_else(|error| panic!("raw 被動監聽失敗（需 CAP_NET_RAW）：{error}"));

    println!("本機 {local}、網段 {network} → 被動 sender {senders:?}");
    for (address, mac) in &senders {
        assert_ne!(*address, local, "不得回報本機自身位址：{address}");
        assert!(is_mac_like(mac), "MAC 格式不合理：{mac}");
    }
    if senders.is_empty() {
        println!("（本次窗內無 ARP sender；設備閒置或權限不足皆可能）");
    }
}

#[test]
#[ignore = "需要真機網路（與目標網段同 L2）與 CAP_NET_RAW"]
fn passive_listen_and_active_sweep_run_concurrently() {
    let (local, network) = local_network();

    // 被動監聽在背景執行 3 秒；主執行緒同時對目標送 ARP 請求（兩者並行）。
    let passive = std::thread::spawn(move || {
        asset_nest_agent::probe::passive_observe(&network, Duration::from_secs(3))
    });

    let target = default_gateway().unwrap_or(local);
    let result = asset_nest_agent::probe::probe_targets(&network, &[target], 1_000)
        .unwrap_or_else(|error| panic!("raw 主動探測失敗（需 CAP_NET_RAW）：{error}"));
    let senders = passive
        .join()
        .expect("被動監聽工作")
        .unwrap_or_else(|error| panic!("raw 被動監聽失敗：{error}"));

    println!(
        "主動目標 {target} → 回應 {:?}；同時被動 sender {senders:?}",
        result.seen
    );
    assert_eq!(result.checked, vec![target], "主動掃描照常送出");
    for (address, mac) in &senders {
        assert_ne!(*address, local, "不得回報本機自身位址：{address}");
        assert!(is_mac_like(mac), "MAC 格式不合理：{mac}");
    }
}

/// 以 UDP connect 取得對外路由所用的本機位址，並以 /24 近似其網段。
///
/// `connect` 不送任何封包；無 netmask 可讀時以 /24 近似（僅影響網段範圍）。
fn local_network() -> (Ipv4Addr, Ipv4Net) {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").expect("綁定 UDP socket");
    socket
        .connect("1.1.1.1:9")
        .expect("建立路由查詢（connect 不送封包）");
    let local = match socket.local_addr().expect("讀取本機位址").ip() {
        IpAddr::V4(address) if !address.is_loopback() => address,
        other => panic!("找不到對外 IPv4 介面（{other}）；本測試需與目標網段同 L2 的網路"),
    };
    let network = Ipv4Net::new(local, 24).expect("合法 /24").trunc();
    (local, network)
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
