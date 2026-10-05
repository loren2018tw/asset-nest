//! 探測邊界：本機同 L2 判定與 ARP 探測（見 spec §探測邊界、ADR-0015）。
//!
//! [`Prober`] 以 `Arc<dyn Prober + Send + Sync>` 掛在 [`crate::AppState`]，
//! 測試注入 stub，任何碰真實網路者不進自動測試。實作策略（見 ADR-0015）：
//!
//! - `raw`：Linux `AF_PACKET` raw socket 自建／解析 ARP 框架（僅依賴 `libc`，
//!   不依賴 libpcap）；需 `CAP_NET_RAW`。
//! - `unprivileged`：對目標丟 UDP 觸發 kernel ARP 解析，再讀 `/proc/net/arp`
//!   （重用 [`crate::peer::mac_from_arp_table`]）。
//! - `auto`（預設）：先 raw；遇權限問題（EPERM／EACCES）或環境不可用時記錄
//!   一次警告並降級 unprivileged。

use std::net::Ipv4Addr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use ipnet::Ipv4Net;

use crate::subnets::Subnet;

/// 正規化後的 MAC 位址字串（小寫冒號格式，如 `aa:bb:cc:dd:ee:ff`）。
pub type Mac = String;

/// 探測模式；由 `OBSERVATION_PROBE_MODE` 設定（見 spec §環境設定）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProbeMode {
    /// 先 raw，失敗時降級 unprivileged（預設）。
    #[default]
    Auto,
    /// 只使用 raw（Linux `AF_PACKET`）；不可用時該次探測回空集合。
    Raw,
    /// 只使用零權限降級（UDP 觸發＋讀 ARP 表）。
    Unprivileged,
}

impl ProbeMode {
    /// 由環境設定字串解析；未知值回傳 `None`（由 [`crate::config::Config`] 拒絕）。
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "raw" => Some(Self::Raw),
            "unprivileged" => Some(Self::Unprivileged),
            _ => None,
        }
    }
}

/// 探測邊界：觀測讀取端與掃描服務以注入的實作判定本機可觀測性並執行探測。
pub trait Prober: Send + Sync {
    /// 本機是否有介面位址落在該 v4 子網（判定同 L2）。
    ///
    /// v6 網段與列舉失敗一律回 `false`（見 ADR-0015）。
    fn is_local(&self, subnet: &Subnet) -> bool;

    /// 對 `targets` 發 ARP 請求並收集回應（回覆窗約 2 秒，實作可調）。
    ///
    /// 回傳（位址、正規化小寫 MAC）；未回應者不出現。阻塞式；呼叫端以
    /// `spawn_blocking` 執行（見 [`crate::observation::run_quick`]）。
    fn probe(&self, subnet: &Subnet, targets: &[Ipv4Addr]) -> Vec<(Ipv4Addr, Mac)>;
}

/// raw 模式的 ARP 回覆窗；送出全部請求後被動收集。
const RAW_REPLY_WINDOW: Duration = Duration::from_secs(2);
/// raw socket 單次 `recv` 的等待上限（輪詢至回覆窗結束）。
const RAW_RECV_POLL: Duration = Duration::from_millis(200);
/// unprivileged 模式送出 UDP 後等待 kernel ARP 解析的簡短時間。
const UNPRIVILEGED_WAIT: Duration = Duration::from_millis(250);

/// 預設實作：raw／unprivileged 雙模式（見 ADR-0015）。
///
/// 不依賴 libpcap；非 Linux 建置的 raw 一律不可用（自動降級）。
#[derive(Debug, Default)]
pub struct SystemProber {
    mode: ProbeMode,
    /// 降級警告只記一次，避免每輪掃描洗版。
    fallback_warned: AtomicBool,
}

impl SystemProber {
    pub fn new() -> Self {
        Self::with_mode(ProbeMode::Auto)
    }

    /// 以指定模式建立（`main` 由 `OBSERVATION_PROBE_MODE` 帶入）。
    pub fn with_mode(mode: ProbeMode) -> Self {
        Self {
            mode,
            fallback_warned: AtomicBool::new(false),
        }
    }

    /// 降級警告僅記錄一次。
    fn warn_fallback_once(&self, reason: &RawProbeError) {
        if self.fallback_warned.swap(true, Ordering::Relaxed) {
            return;
        }
        tracing::warn!(reason = %reason, "raw ARP 不可用，改用零權限降級模式（UDP 觸發＋/proc/net/arp）");
    }
}

impl Prober for SystemProber {
    fn is_local(&self, subnet: &Subnet) -> bool {
        let Ok(network) = subnet.cidr.parse::<Ipv4Net>() else {
            return false; // v6 或非法 CIDR：一律非同 L2
        };
        contains_address(&network, &local_ipv4_addresses())
    }

    fn probe(&self, subnet: &Subnet, targets: &[Ipv4Addr]) -> Vec<(Ipv4Addr, Mac)> {
        if targets.is_empty() {
            return Vec::new();
        }

        match self.mode {
            ProbeMode::Raw => match raw_probe(subnet, targets) {
                Ok(responses) => responses,
                Err(error) => {
                    self.warn_fallback_once(&error);
                    Vec::new()
                }
            },
            ProbeMode::Unprivileged => unprivileged_probe(targets),
            ProbeMode::Auto => match raw_probe(subnet, targets) {
                Ok(responses) => responses,
                Err(error) => {
                    self.warn_fallback_once(&error);
                    unprivileged_probe(targets)
                }
            },
        }
    }
}

/// raw 模式的失敗原因；權限問題與環境不可用區分以供降級決策與訊息。
#[derive(Debug)]
enum RawProbeError {
    /// `AF_PACKET` socket 建立被拒（EPERM／EACCES；缺 CAP_NET_RAW）。
    Permission,
    /// 找不到介面、讀不到 MAC、CIDR 非 v4 等環境問題。
    Unusable(String),
}

impl std::fmt::Display for RawProbeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Permission => write!(formatter, "需要 CAP_NET_RAW 權限"),
            Self::Unusable(reason) => write!(formatter, "{reason}"),
        }
    }
}

/// 由作業系統錯誤區分權限問題；其他一律視為環境不可用。
fn classify_io_error(error: std::io::Error) -> RawProbeError {
    match error.raw_os_error() {
        Some(libc::EPERM) | Some(libc::EACCES) => RawProbeError::Permission,
        _ => RawProbeError::Unusable(error.to_string()),
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

/// 正規化 MAC 文字為小寫冒號格式；格式不符回 `None`。
///
/// 接受 `-` 或 `:` 分隔的 6／8 組兩位十六進位（大小寫不拘；Ethernet 與
/// EUI-64，比照 Kea `hw-address` 格式）。
pub fn normalize_mac(text: &str) -> Option<Mac> {
    let normalized = text.trim().to_ascii_lowercase().replace('-', ":");
    let groups: Vec<&str> = normalized.split(':').collect();
    if !matches!(groups.len(), 6 | 8)
        || !groups
            .iter()
            .all(|group| group.len() == 2 && group.chars().all(|c| c.is_ascii_hexdigit()))
    {
        return None;
    }
    Some(groups.join(":"))
}

/// 由位元組格式化 MAC（小寫冒號格式）；供 ARP 框架解析。
fn format_mac(bytes: &[u8]) -> Mac {
    bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join(":")
}

/// 由 MAC 文字取出 6 個位元組；格式不符回 `None`（純函式）。
fn mac_octets(mac: &str) -> Option<[u8; 6]> {
    let normalized = normalize_mac(mac)?;
    let mut octets = [0u8; 6];
    let mut groups = normalized.split(':');
    for octet in &mut octets {
        let group = groups.next()?;
        *octet = u8::from_str_radix(group, 16).ok()?;
    }
    groups.next().is_none().then_some(octets)
}

/// 組出 ARP 請求 Ethernet 框架（純函式；見 spec §探測邊界）。
///
/// 版面：Ethernet（broadcast dest、`sender_mac`、EtherType 0x0806）
/// ＋ ARP（htype 1、ptype 0x0800、hlen 6、plen 4、opcode 1、sender、target）。
/// `sender_mac` 格式不符回 `None`。
pub fn build_arp_request(
    sender_mac: &str,
    sender_ip: Ipv4Addr,
    target_ip: Ipv4Addr,
) -> Option<Vec<u8>> {
    let octets = mac_octets(sender_mac)?;

    let mut frame = Vec::with_capacity(42);
    frame.extend_from_slice(&[0xff; 6]); // 目的：broadcast
    frame.extend_from_slice(&octets);
    frame.extend_from_slice(&0x0806u16.to_be_bytes()); // EtherType ARP
    frame.extend_from_slice(&1u16.to_be_bytes()); // htype：Ethernet
    frame.extend_from_slice(&0x0800u16.to_be_bytes()); // ptype：IPv4
    frame.push(6); // hlen
    frame.push(4); // plen
    frame.extend_from_slice(&1u16.to_be_bytes()); // opcode：request
    frame.extend_from_slice(&octets);
    frame.extend_from_slice(&sender_ip.octets());
    frame.extend_from_slice(&[0u8; 6]); // target MAC 未知
    frame.extend_from_slice(&target_ip.octets());
    Some(frame)
}

/// 解析 ARP 回覆框架（純函式）；非 ARP 回覆或格式不符回 `None`。
///
/// 回傳（sender protocol address、sender hardware address）。
pub fn parse_arp_reply(frame: &[u8]) -> Option<(Ipv4Addr, Mac)> {
    // Ethernet 標頭 14 ＋ ARP（IPv4）28。
    if frame.len() < 42 {
        return None;
    }
    if u16::from_be_bytes([frame[12], frame[13]]) != 0x0806 {
        return None; // 非 ARP
    }

    let arp = &frame[14..];
    if u16::from_be_bytes([arp[0], arp[1]]) != 1 || u16::from_be_bytes([arp[2], arp[3]]) != 0x0800 {
        return None; // 非 Ethernet／IPv4
    }
    if arp[4] != 6 || arp[5] != 4 {
        return None; // 非預期長度
    }
    if u16::from_be_bytes([arp[6], arp[7]]) != 2 {
        return None; // 僅解析回覆（opcode 2）
    }

    let sender_mac = format_mac(&arp[8..14]);
    let sender_ip = Ipv4Addr::new(arp[14], arp[15], arp[16], arp[17]);
    Some((sender_ip, sender_mac))
}

/// 讀取介面 MAC（`/sys/class/net/<if>/address`）；正規化失敗回 `None`。
#[cfg(target_os = "linux")]
fn read_interface_mac(interface: &str) -> Option<Mac> {
    let text = std::fs::read_to_string(format!("/sys/class/net/{interface}/address")).ok()?;
    normalize_mac(&text)
}

/// 找出第一個位址落在 `network` 內的本機介面（名稱、位址）。
#[cfg(target_os = "linux")]
fn local_interface(network: &Ipv4Net) -> Option<(String, Ipv4Addr)> {
    // SAFETY: 比照 `local_ipv4_addresses`；鏈結清單於 freeifaddrs 前維持有效。
    unsafe {
        let mut list: *mut libc::ifaddrs = std::ptr::null_mut();
        if libc::getifaddrs(&mut list) != 0 {
            return None;
        }

        let mut found = None;
        let mut current = list;
        while !current.is_null() {
            let interface = &*current;
            if !interface.ifa_addr.is_null()
                && (*interface.ifa_addr).sa_family as libc::c_int == libc::AF_INET
            {
                let address = &*(interface.ifa_addr as *const libc::sockaddr_in);
                let address = Ipv4Addr::from(u32::from_be(address.sin_addr.s_addr));
                if network.contains(&address) {
                    let name = std::ffi::CStr::from_ptr(interface.ifa_name)
                        .to_string_lossy()
                        .into_owned();
                    found = Some((name, address));
                    break;
                }
            }
            current = interface.ifa_next;
        }

        libc::freeifaddrs(list);
        found
    }
}

/// 已綁定介面的 `AF_PACKET` raw socket；解構時關閉 fd。
#[cfg(target_os = "linux")]
struct RawSocket {
    fd: libc::c_int,
    ifindex: libc::c_int,
}

#[cfg(target_os = "linux")]
impl Drop for RawSocket {
    fn drop(&mut self) {
        // SAFETY: fd 由本結構獨佔，關閉一次。
        unsafe {
            libc::close(self.fd);
        }
    }
}

/// 建立綁定介面的 ARP raw socket；權限被拒回 [`RawProbeError::Permission`]。
#[cfg(target_os = "linux")]
fn open_arp_socket(interface: &str) -> Result<RawSocket, RawProbeError> {
    let name = std::ffi::CString::new(interface)
        .map_err(|_| RawProbeError::Unusable("介面名稱含 NUL 字元".to_string()))?;

    // SAFETY: 依 socket(2)／if_nametoindex(3) 契約。
    let ifindex = unsafe { libc::if_nametoindex(name.as_ptr()) };
    if ifindex == 0 {
        return Err(RawProbeError::Unusable(format!("找不到介面 {interface}")));
    }

    // SAFETY: AF_PACKET/SOCK_RAW 僅建立 socket；失敗回 -1。
    let fd = unsafe {
        libc::socket(
            libc::AF_PACKET,
            libc::SOCK_RAW,
            (libc::ETH_P_ARP as u16).to_be() as libc::c_int,
        )
    };
    if fd < 0 {
        return Err(classify_io_error(std::io::Error::last_os_error()));
    }

    let address = libc::sockaddr_ll {
        sll_family: libc::AF_PACKET as u16,
        sll_protocol: (libc::ETH_P_ARP as u16).to_be(),
        sll_ifindex: ifindex as libc::c_int,
        sll_hatype: 0,
        sll_pkttype: 0,
        sll_halen: 0,
        sll_addr: [0; 8],
    };

    // SAFETY: address 為合法 sockaddr_ll；fd 已建立。
    let bound = unsafe {
        libc::bind(
            fd,
            &address as *const libc::sockaddr_ll as *const libc::sockaddr,
            std::mem::size_of::<libc::sockaddr_ll>() as libc::socklen_t,
        )
    };
    if bound != 0 {
        let error = std::io::Error::last_os_error();
        // SAFETY: fd 建立成功且尚未交給 RawSocket。
        unsafe {
            libc::close(fd);
        }
        return Err(classify_io_error(error));
    }

    // 單次 recv 的等待上限；收集迴圈輪詢至回覆窗結束。
    let timeout = libc::timeval {
        tv_sec: 0,
        tv_usec: RAW_RECV_POLL.as_micros() as libc::suseconds_t,
    };
    // SAFETY: timeout 為合法 timeval；fd 有效。
    let configured = unsafe {
        libc::setsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_RCVTIMEO,
            &timeout as *const libc::timeval as *const libc::c_void,
            std::mem::size_of::<libc::timeval>() as libc::socklen_t,
        )
    };
    if configured != 0 {
        let error = std::io::Error::last_os_error();
        // SAFETY: fd 建立成功且尚未交給 RawSocket。
        unsafe {
            libc::close(fd);
        }
        return Err(classify_io_error(error));
    }

    Ok(RawSocket {
        fd,
        ifindex: ifindex as libc::c_int,
    })
}

/// 由綁定的 raw socket 送出單一 Ethernet 框架（廣播）。
#[cfg(target_os = "linux")]
fn send_frame(socket: &RawSocket, frame: &[u8]) -> Result<(), RawProbeError> {
    let mut address: libc::sockaddr_ll = unsafe { std::mem::zeroed() };
    address.sll_family = libc::AF_PACKET as u16;
    address.sll_protocol = (libc::ETH_P_ARP as u16).to_be();
    address.sll_ifindex = socket.ifindex;
    address.sll_halen = 6;
    address.sll_addr[..6].copy_from_slice(&[0xff; 6]);

    // SAFETY: frame 為本函式持有的緩衝區；address 為合法 sockaddr_ll。
    let sent = unsafe {
        libc::sendto(
            socket.fd,
            frame.as_ptr() as *const libc::c_void,
            frame.len(),
            0,
            &address as *const libc::sockaddr_ll as *const libc::sockaddr,
            std::mem::size_of::<libc::sockaddr_ll>() as libc::socklen_t,
        )
    };
    if sent < 0 {
        return Err(classify_io_error(std::io::Error::last_os_error()));
    }
    Ok(())
}

/// 收集回覆窗內的 ARP 回覆；僅保留目標集合內、每個位址第一筆回應。
#[cfg(target_os = "linux")]
fn collect_replies(
    socket: &RawSocket,
    targets: &[Ipv4Addr],
    window: Duration,
) -> Vec<(Ipv4Addr, Mac)> {
    use std::time::Instant;

    let deadline = Instant::now() + window;
    let mut buffer = [0u8; 2048];
    let mut responses: Vec<(Ipv4Addr, Mac)> = Vec::new();

    while Instant::now() < deadline {
        // SAFETY: buffer 為本函式持有的可寫緩衝區；fd 有效。
        let received = unsafe {
            libc::recv(
                socket.fd,
                buffer.as_mut_ptr() as *mut libc::c_void,
                buffer.len(),
                0,
            )
        };
        if received < 0 {
            let error = std::io::Error::last_os_error();
            if matches!(
                error.kind(),
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
            ) {
                continue; // 等待窗逾時：輪詢至回覆窗結束
            }
            break;
        }
        if received == 0 {
            continue;
        }

        let frame = &buffer[..received as usize];
        if let Some((address, mac)) = parse_arp_reply(frame) {
            if targets.contains(&address) && !responses.iter().any(|(known, _)| *known == address) {
                responses.push((address, mac));
            }
        }
    }

    responses.sort_by_key(|(address, _)| *address);
    responses
}

/// raw 模式探測；Linux 以 `AF_PACKET` 自建 ARP 請求並收集回覆。
#[cfg(target_os = "linux")]
fn raw_probe(subnet: &Subnet, targets: &[Ipv4Addr]) -> Result<Vec<(Ipv4Addr, Mac)>, RawProbeError> {
    let network: Ipv4Net = subnet
        .cidr
        .parse()
        .map_err(|_| RawProbeError::Unusable("網段 CIDR 非 IPv4".to_string()))?;
    let (interface, sender_ip) = local_interface(&network)
        .ok_or_else(|| RawProbeError::Unusable("找不到落在網段內的本機介面".to_string()))?;
    let sender_mac = read_interface_mac(&interface)
        .ok_or_else(|| RawProbeError::Unusable(format!("讀取介面 {interface} MAC 失敗")))?;

    let socket = open_arp_socket(&interface)?;
    for target in targets {
        let frame = build_arp_request(&sender_mac, sender_ip, *target)
            .ok_or_else(|| RawProbeError::Unusable("本機介面 MAC 格式錯誤".to_string()))?;
        send_frame(&socket, &frame)?;
    }

    Ok(collect_replies(&socket, targets, RAW_REPLY_WINDOW))
}

/// 非 Linux：raw 一律不可用（見 ADR-0015）。
#[cfg(not(target_os = "linux"))]
fn raw_probe(
    _subnet: &Subnet,
    _targets: &[Ipv4Addr],
) -> Result<Vec<(Ipv4Addr, Mac)>, RawProbeError> {
    Err(RawProbeError::Unusable(
        "非 Linux 不支援 raw ARP 探測".to_string(),
    ))
}

/// 零權限降級：對每個目標丟 UDP 觸發 kernel ARP 解析，再讀 `/proc/net/arp`。
fn unprivileged_probe(targets: &[Ipv4Addr]) -> Vec<(Ipv4Addr, Mac)> {
    // 觸發 ARP：kernel 於送出前解析目的 MAC；單一 socket 共用。
    if let Ok(socket) = std::net::UdpSocket::bind("0.0.0.0:0") {
        for target in targets {
            // 目的埠無關緊要（UDP 不回覆）；失敗（如無路由）不影響後續讀表。
            let _ = socket.send_to(&[0u8; 1], std::net::SocketAddr::from((*target, 9)));
        }
    }

    std::thread::sleep(UNPRIVILEGED_WAIT);

    let Ok(table) = std::fs::read_to_string("/proc/net/arp") else {
        return Vec::new();
    };
    targets
        .iter()
        .filter_map(|target| {
            crate::peer::mac_from_arp_table(&table, &target.to_string())
                .and_then(|mac| normalize_mac(&mac))
                .map(|mac| (*target, mac))
        })
        .collect()
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
            observed: false,
            pools: Vec::new(),
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    /// 測試用位址。
    fn addr(text: &str) -> Ipv4Addr {
        text.parse().expect("合法位址")
    }

    /// 以 ARP 回覆版面組出框架：借 `build_arp_request` 的 sender／target 欄位，
    /// 改寫 Ethernet 目的 MAC 與 opcode（純測試輔助）。
    fn reply_frame(
        responder_mac: &str,
        responder_ip: Ipv4Addr,
        requester_mac: &str,
        requester_ip: Ipv4Addr,
    ) -> Vec<u8> {
        let mut frame =
            build_arp_request(responder_mac, responder_ip, requester_ip).expect("合法 sender MAC");
        frame[0..6].copy_from_slice(&mac_octets(requester_mac).expect("合法 requester MAC"));
        frame[20..22].copy_from_slice(&2u16.to_be_bytes()); // opcode：reply
        frame
    }

    #[test]
    fn presence_containment_matches_any_listed_address() {
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

    #[test]
    fn probe_mode_parses_known_values_case_insensitively() {
        assert_eq!(ProbeMode::parse("auto"), Some(ProbeMode::Auto));
        assert_eq!(ProbeMode::parse(" RAW "), Some(ProbeMode::Raw));
        assert_eq!(
            ProbeMode::parse("Unprivileged"),
            Some(ProbeMode::Unprivileged)
        );
        assert_eq!(ProbeMode::parse("pcap"), None);
        assert_eq!(ProbeMode::default(), ProbeMode::Auto);
    }

    #[test]
    fn normalize_mac_accepts_colons_and_dashes_and_lowercases() {
        assert_eq!(
            normalize_mac("AA:BB:CC:dd:ee:ff"),
            Some("aa:bb:cc:dd:ee:ff".to_string())
        );
        assert_eq!(
            normalize_mac("AA-BB-CC-DD-EE-FF\n"),
            Some("aa:bb:cc:dd:ee:ff".to_string())
        );
        assert_eq!(normalize_mac("aa:bb:cc:dd:ee"), None, "群組數不足");
        assert_eq!(normalize_mac("aa:bb:cc:dd:ee:g1"), None, "非十六進位");
        assert_eq!(normalize_mac(""), None);
    }

    #[test]
    fn arp_request_frame_has_expected_layout() {
        let frame = build_arp_request("aa:bb:cc:dd:ee:01", addr("10.0.0.2"), addr("10.0.0.7"))
            .expect("合法 MAC");
        assert_eq!(frame.len(), 42);
        assert_eq!(&frame[0..6], &[0xff; 6], "目的為 broadcast");
        assert_eq!(&frame[6..12], &[0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0x01]);
        assert_eq!(u16::from_be_bytes([frame[12], frame[13]]), 0x0806);
        assert_eq!(u16::from_be_bytes([frame[14], frame[15]]), 1, "htype");
        assert_eq!(u16::from_be_bytes([frame[16], frame[17]]), 0x0800, "ptype");
        assert_eq!(
            u16::from_be_bytes([frame[20], frame[21]]),
            1,
            "opcode request"
        );
        assert_eq!(&frame[22..28], &[0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0x01]);
        assert_eq!(&frame[28..32], &[10, 0, 0, 2], "sender IP");
        assert_eq!(&frame[32..38], &[0; 6], "target MAC 未知");
        assert_eq!(&frame[38..42], &[10, 0, 0, 7], "target IP");

        assert_eq!(
            build_arp_request("not-a-mac", addr("10.0.0.2"), addr("10.0.0.7")),
            None
        );
    }

    #[test]
    fn arp_reply_round_trips_and_requests_are_ignored() {
        let request = build_arp_request("aa:bb:cc:dd:ee:01", addr("10.0.0.2"), addr("10.0.0.7"))
            .expect("合法 MAC");
        assert_eq!(
            parse_arp_reply(&request),
            None,
            "請求（opcode 1）不得視為回覆"
        );

        let reply = reply_frame(
            "0A:0B:0C:0D:0E:0F",
            addr("10.0.0.7"),
            "aa:bb:cc:dd:ee:01",
            addr("10.0.0.2"),
        );
        assert_eq!(
            parse_arp_reply(&reply),
            Some((addr("10.0.0.7"), "0a:0b:0c:0d:0e:0f".to_string())),
            "回覆解析出 responder 的位址與正規化 MAC"
        );
    }

    #[test]
    fn arp_reply_parser_rejects_truncated_and_foreign_frames() {
        let reply = reply_frame(
            "aa:bb:cc:dd:ee:02",
            addr("10.0.0.9"),
            "aa:bb:cc:dd:ee:01",
            addr("10.0.0.2"),
        );
        assert_eq!(parse_arp_reply(&reply[..41]), None, "長度不足");
        assert_eq!(parse_arp_reply(&[]), None);

        let mut ipv4 = reply.clone();
        ipv4[12..14].copy_from_slice(&0x0800u16.to_be_bytes());
        assert_eq!(parse_arp_reply(&ipv4), None, "非 ARP EtherType");

        let mut bad_length = reply.clone();
        bad_length[18] = 8; // hlen 非 6
        assert_eq!(parse_arp_reply(&bad_length), None, "硬體長度不符");
    }
}
