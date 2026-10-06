//! raw ARP 探測（自 backend `probe.rs` 複製；後端本體保留至票 08）。
//!
//! 以 Linux `AF_PACKET`／`ETH_P_ARP` 自建與解析 ARP 框架（僅依賴 `libc`，
//! 不依賴 libpcap）；需 `CAP_NET_RAW`。模組另有介面列舉（網段自動偵測與
//! 探測介面選擇）與被動監聽（持續迴圈 [`passive_listen`] 與短窗
//! [`passive_observe`]；見票 05、spec §持續被動）。
//!
//! 主動探測語意沿用後端：「送一批、收約 2 秒回覆窗」，批與批的開始時間
//! 相距約 1 秒以貼近 `rate_pps`。
//!
//! 被動監聽**只收不送**：解析 opcode 1／2 的 sender 並套用排除規則
//! （[`valid_passive_sender`]）；持續迴圈由 caller 提供停止條件與逐筆回呼。

use std::net::Ipv4Addr;
use std::time::Duration;

use ipnet::Ipv4Net;

use crate::host_range::HostRange;

/// 正規化後的 MAC 位址字串（小寫冒號格式，如 `aa:bb:cc:dd:ee:ff`）。
pub type Mac = String;

/// 主動探測的回覆窗硬上限；送出整批後被動收集。
pub const RAW_REPLY_WINDOW: Duration = Duration::from_secs(2);
/// 距最後一幀超過此時間即提早結束回覆窗（無回應時不空等硬上限）。
pub const RAW_IDLE_EXIT: Duration = Duration::from_millis(500);
/// raw socket 單次 `recv` 的等待上限（輪詢以判斷是否提早結束）。
pub const RAW_RECV_POLL: Duration = Duration::from_millis(200);

/// 一次主動掃描的結果（見 spec §週期掃描）。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SweepResult {
    /// 全部送出的目標位址（正常路徑即整批 host；見票 04）。
    pub checked: Vec<Ipv4Addr>,
    /// 有 ARP 回應的位址與正規化小寫冒號 MAC。
    pub seen: Vec<(Ipv4Addr, Mac)>,
}

/// raw 探測的失敗原因；權限問題與環境不可用區分以供訊息說明。
#[derive(Debug)]
pub enum ProbeError {
    /// `AF_PACKET` socket 建立被拒（EPERM／EACCES；缺 CAP_NET_RAW）。
    Permission,
    /// 找不到介面、讀不到 MAC、非 Linux 等環境問題。
    Unusable(String),
}

impl std::fmt::Display for ProbeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Permission => write!(formatter, "需要 CAP_NET_RAW 權限"),
            Self::Unusable(reason) => write!(formatter, "{reason}"),
        }
    }
}

impl std::error::Error for ProbeError {}

/// 由作業系統錯誤區分權限問題；其他一律視為環境不可用。
fn classify_io_error(error: std::io::Error) -> ProbeError {
    match error.raw_os_error() {
        Some(libc::EPERM) | Some(libc::EACCES) => ProbeError::Permission,
        _ => ProbeError::Unusable(error.to_string()),
    }
}

/// 列舉本機介面的 IPv4 位址；列舉失敗回空清單。
#[cfg(target_os = "linux")]
pub fn local_ipv4_addresses() -> Vec<Ipv4Addr> {
    let mut addresses = Vec::new();

    // SAFETY: 依 getifaddrs(3) 契約——`list` 成功時由函式配置、
    // 由 freeifaddrs 釋放；走訪期間鏈結清單維持有效。
    unsafe {
        let mut list: *mut libc::ifaddrs = std::ptr::null_mut();
        if libc::getifaddrs(&mut list) != 0 {
            tracing::debug!("列舉本機網路介面失敗（getifaddrs）");
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

/// 非 Linux：不支援 `getifaddrs`，一律回空清單。
#[cfg(not(target_os = "linux"))]
pub fn local_ipv4_addresses() -> Vec<Ipv4Addr> {
    Vec::new()
}

/// 列舉本機非 loopback、已啟動（IFF_UP）介面的 IPv4 網段（供網段自動偵測）。
///
/// 由位址與 netmask 推導網段（host bits 收斂）；列舉失敗或無候選回空清單。
#[cfg(target_os = "linux")]
pub fn interface_networks() -> Vec<Ipv4Net> {
    let mut networks = Vec::new();

    // SAFETY: 比照 `local_ipv4_addresses`；鏈結清單於 freeifaddrs 前維持有效。
    unsafe {
        let mut list: *mut libc::ifaddrs = std::ptr::null_mut();
        if libc::getifaddrs(&mut list) != 0 {
            tracing::debug!("列舉本機網路介面失敗（getifaddrs）");
            return networks;
        }

        let mut current = list;
        while !current.is_null() {
            let interface = &*current;
            let is_up = interface.ifa_flags as libc::c_int & libc::IFF_UP != 0;
            if is_up
                && !interface.ifa_addr.is_null()
                && !interface.ifa_netmask.is_null()
                && (*interface.ifa_addr).sa_family as libc::c_int == libc::AF_INET
            {
                let address = &*(interface.ifa_addr as *const libc::sockaddr_in);
                let address = Ipv4Addr::from(u32::from_be(address.sin_addr.s_addr));
                if !address.is_loopback() {
                    let netmask = &*(interface.ifa_netmask as *const libc::sockaddr_in);
                    let netmask = Ipv4Addr::from(u32::from_be(netmask.sin_addr.s_addr));
                    if let Ok(network) = Ipv4Net::with_netmask(address, netmask) {
                        networks.push(network.trunc());
                    }
                }
            }
            current = interface.ifa_next;
        }

        libc::freeifaddrs(list);
    }

    networks
}

/// 非 Linux：不支援 `getifaddrs`，一律回空清單。
#[cfg(not(target_os = "linux"))]
pub fn interface_networks() -> Vec<Ipv4Net> {
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

/// 組出 ARP 請求 Ethernet 框架（純函式）。
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

/// 解析 ARP 框架的共用欄位（純函式）；非 ARP 或格式不符回 `None`。
///
/// 回傳（opcode、sender protocol address、sender hardware address）。
fn parse_arp_frame(frame: &[u8]) -> Option<(u16, Ipv4Addr, Mac)> {
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

    let opcode = u16::from_be_bytes([arp[6], arp[7]]);
    let sender_mac = format_mac(&arp[8..14]);
    let sender_ip = Ipv4Addr::new(arp[14], arp[15], arp[16], arp[17]);
    Some((opcode, sender_ip, sender_mac))
}

/// 解析 ARP 回覆框架（純函式）；非 ARP 回覆（opcode 2）或格式不符回 `None`。
///
/// 回傳（sender protocol address、sender hardware address）。主動探測專用；
/// 被動監聽請用 [`parse_arp_sender`]。
pub fn parse_arp_reply(frame: &[u8]) -> Option<(Ipv4Addr, Mac)> {
    match parse_arp_frame(frame)? {
        (2, sender_ip, sender_mac) => Some((sender_ip, sender_mac)),
        _ => None,
    }
}

/// 解析 ARP request（opcode 1）／reply（opcode 2）的 sender（純函式）。
///
/// 被動監聽（票 05）用：兩種 opcode 的 sender IP／MAC 都是「有人在用該
/// 位址」的證據；其他 opcode 或格式不符回 `None`。
pub fn parse_arp_sender(frame: &[u8]) -> Option<(Ipv4Addr, Mac)> {
    match parse_arp_frame(frame)? {
        (1 | 2, sender_ip, sender_mac) => Some((sender_ip, sender_mac)),
        _ => None,
    }
}

/// 被動 sender 是否可記錄（純函式；見 ADR-0017；票 05 使用）。
///
/// 排除 `0.0.0.0`、multicast、有限廣播（`255.255.255.255` 與網段廣播）以及
/// 本機介面自身的位址／MAC。MAC 比較不分大小寫。
pub fn valid_passive_sender(
    address: Ipv4Addr,
    mac: &str,
    local_ip: Ipv4Addr,
    local_mac: &str,
    network_broadcast: Ipv4Addr,
) -> bool {
    !address.is_unspecified()
        && !address.is_multicast()
        && !address.is_broadcast()
        && address != network_broadcast
        && address != local_ip
        && !mac.eq_ignore_ascii_case(local_mac)
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

/// 建立綁定介面的 ARP raw socket；權限被拒回 [`ProbeError::Permission`]。
#[cfg(target_os = "linux")]
fn open_arp_socket(interface: &str) -> Result<RawSocket, ProbeError> {
    let name = std::ffi::CString::new(interface)
        .map_err(|_| ProbeError::Unusable("介面名稱含 NUL 字元".to_string()))?;

    // SAFETY: 依 socket(2)／if_nametoindex(3) 契約。
    let ifindex = unsafe { libc::if_nametoindex(name.as_ptr()) };
    if ifindex == 0 {
        return Err(ProbeError::Unusable(format!("找不到介面 {interface}")));
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
fn send_frame(socket: &RawSocket, frame: &[u8]) -> Result<(), ProbeError> {
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

/// 回覆窗是否可結束（純函式；供單元測試）。
///
/// 任一條件成立即結束：全部目標已回應、距最後一幀已達 `idle`、已達硬上限
/// `window`。`since_last_frame` 由呼叫端維護（尚無幀時自收集開始起算）。
fn reply_window_done(
    elapsed: Duration,
    since_last_frame: Duration,
    answered: usize,
    target_count: usize,
    window: Duration,
    idle: Duration,
) -> bool {
    answered >= target_count || since_last_frame >= idle || elapsed >= window
}

/// 收集回覆窗內的 ARP 回覆；僅保留目標集合內、每個位址第一筆回應。
///
/// 全部目標回應、閒置 [`RAW_IDLE_EXIT`] 或達硬上限 [`RAW_REPLY_WINDOW`]
/// 時結束（決策見 [`reply_window_done`]）。
#[cfg(target_os = "linux")]
fn collect_replies(
    socket: &RawSocket,
    targets: &[Ipv4Addr],
    window: Duration,
) -> Vec<(Ipv4Addr, Mac)> {
    use std::time::Instant;

    let started = Instant::now();
    let mut last_frame = started;
    let mut buffer = [0u8; 2048];
    let mut responses: Vec<(Ipv4Addr, Mac)> = Vec::new();

    loop {
        if reply_window_done(
            started.elapsed(),
            last_frame.elapsed(),
            responses.len(),
            targets.len(),
            window,
            RAW_IDLE_EXIT,
        ) {
            break;
        }

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
                continue; // 等待窗逾時：回到迴圈開頭判斷是否提早結束
            }
            break;
        }
        if received == 0 {
            continue;
        }
        last_frame = Instant::now();

        let frame = &buffer[..received as usize];
        if let Some((address, mac)) = parse_arp_reply(frame)
            && targets.contains(&address)
            && !responses.iter().any(|(known, _)| *known == address)
        {
            responses.push((address, mac));
        }
    }

    responses.sort_by_key(|(address, _)| *address);
    responses
}

/// 批與批的剩餘等待（純函式）：批開始時間相距約 `interval`；探測耗時
/// `elapsed` 後只補足剩餘時間，耗時已達或超過間隔時不再等待。
fn batch_pacing_delay(elapsed: Duration, interval: Duration) -> Duration {
    interval.saturating_sub(elapsed)
}

/// 對 `targets` 限速主動探測（阻塞式；見 spec §週期掃描）。
///
/// 每批最多 `rate_pps` 個目標：送出整批後以 [`RAW_REPLY_WINDOW`] 收集
/// 回覆，批與批間補足約 1 秒間隔；`checked` 為實際送出的位址。
/// 呼叫端以 `tokio::task::spawn_blocking` 執行（見 [`sweep`]）。
pub fn probe_targets(
    network: &Ipv4Net,
    targets: &[Ipv4Addr],
    rate_pps: u32,
) -> Result<SweepResult, ProbeError> {
    if targets.is_empty() {
        return Ok(SweepResult::default());
    }
    probe_targets_platform(network, targets, rate_pps)
}

/// 平台實作（Linux）：開 raw socket、分批送收。
#[cfg(target_os = "linux")]
fn probe_targets_platform(
    network: &Ipv4Net,
    targets: &[Ipv4Addr],
    rate_pps: u32,
) -> Result<SweepResult, ProbeError> {
    use std::time::Instant;

    let (interface, sender_ip) = local_interface(network)
        .ok_or_else(|| ProbeError::Unusable(format!("找不到落在網段 {network} 內的本機介面")))?;
    let sender_mac = read_interface_mac(&interface)
        .ok_or_else(|| ProbeError::Unusable(format!("讀取介面 {interface} MAC 失敗")))?;

    let socket = open_arp_socket(&interface)?;
    let batch_size = rate_pps.max(1) as usize;
    let batch_count = targets.len().div_ceil(batch_size);
    let mut result = SweepResult {
        checked: Vec::with_capacity(targets.len()),
        seen: Vec::new(),
    };

    for (index, batch) in targets.chunks(batch_size).enumerate() {
        let batch_started = Instant::now();
        for target in batch {
            let frame = build_arp_request(&sender_mac, sender_ip, *target)
                .ok_or_else(|| ProbeError::Unusable("本機介面 MAC 格式錯誤".to_string()))?;
            send_frame(&socket, &frame)?;
            result.checked.push(*target);
        }

        result
            .seen
            .extend(collect_replies(&socket, batch, RAW_REPLY_WINDOW));

        if index + 1 < batch_count {
            std::thread::sleep(batch_pacing_delay(
                batch_started.elapsed(),
                Duration::from_secs(1),
            ));
        }
    }

    result.seen.sort_by_key(|(address, _)| *address);
    Ok(result)
}

/// 非 Linux：raw 一律不可用。
#[cfg(not(target_os = "linux"))]
fn probe_targets_platform(
    _network: &Ipv4Net,
    _targets: &[Ipv4Addr],
    _rate_pps: u32,
) -> Result<SweepResult, ProbeError> {
    Err(ProbeError::Unusable(
        "非 Linux 不支援 raw ARP 探測".to_string(),
    ))
}

/// 對網段全部 host 位址限速主動探測（見 spec §週期掃描）。
///
/// host 列舉沿用 [`HostRange`]（扣 network／broadcast；`/31`、`/32` 全列），
/// 實際送收為阻塞式，於 `spawn_blocking` 執行。
pub async fn sweep(network: Ipv4Net, rate_pps: u32) -> Result<SweepResult, ProbeError> {
    let targets: Vec<Ipv4Addr> = HostRange::of(&network).iter().collect();
    tokio::task::spawn_blocking(move || probe_targets(&network, &targets, rate_pps))
        .await
        .map_err(|error| ProbeError::Unusable(format!("掃描工作失敗：{error}")))?
}

/// 持續被動監聽（阻塞式；只收不送；見 spec §持續被動）。
///
/// 在網段介面上開啟 ARP socket 後持續收訊：每筆合法 sender（opcode 1／2、
/// 排除規則見 [`valid_passive_sender`]）呼叫一次 `on_sender`；`should_stop`
/// 每輪（單次 `recv` 等待上限 [`RAW_RECV_POLL`]）檢查一次，回 `true` 時正常
/// 結束。socket 無法建立或讀取失敗（非逾時）回 [`ProbeError`]。
///
/// 阻塞式；代理常駐路徑以 `tokio::task::spawn_blocking` 執行（見
/// [`crate::passive`]）。不去重、不濾 CIDR：聚合與 CIDR 過濾由呼叫端負責。
pub fn passive_listen(
    network: &Ipv4Net,
    should_stop: impl Fn() -> bool,
    mut on_sender: impl FnMut(Ipv4Addr, Mac),
) -> Result<(), ProbeError> {
    passive_listen_platform(network, &should_stop, &mut on_sender)
}

/// 平台實作（Linux）：開 raw socket 只收不送，逐筆回呼合法 sender。
#[cfg(target_os = "linux")]
fn passive_listen_platform(
    network: &Ipv4Net,
    should_stop: &dyn Fn() -> bool,
    on_sender: &mut dyn FnMut(Ipv4Addr, Mac),
) -> Result<(), ProbeError> {
    let (interface, local_ip) = local_interface(network)
        .ok_or_else(|| ProbeError::Unusable(format!("找不到落在網段 {network} 內的本機介面")))?;
    let local_mac = read_interface_mac(&interface)
        .ok_or_else(|| ProbeError::Unusable(format!("讀取介面 {interface} MAC 失敗")))?;

    let socket = open_arp_socket(&interface)?;
    let broadcast = network.broadcast();
    let mut buffer = [0u8; 2048];

    while !should_stop() {
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
                continue; // 等待窗逾時：回到迴圈開頭檢查是否結束
            }
            return Err(classify_io_error(error));
        }
        if received == 0 {
            continue;
        }

        let frame = &buffer[..received as usize];
        if let Some((address, mac)) = parse_arp_sender(frame)
            && valid_passive_sender(address, &mac, local_ip, &local_mac, broadcast)
        {
            on_sender(address, mac);
        }
    }
    Ok(())
}

/// 非 Linux：raw 一律不可用。
#[cfg(not(target_os = "linux"))]
fn passive_listen_platform(
    _network: &Ipv4Net,
    _should_stop: &dyn Fn() -> bool,
    _on_sender: &mut dyn FnMut(Ipv4Addr, Mac),
) -> Result<(), ProbeError> {
    Err(ProbeError::Unusable(
        "非 Linux 不支援 raw ARP 被動監聽".to_string(),
    ))
}

/// 短窗被動監聽（阻塞式；只收不送）：回傳窗內全部合法 sender。
///
/// 不去重、不濾 CIDR（比照後端語意，供真機 `#[ignore]` 測試與人工檢查）；
/// 代理常駐路徑請用 [`passive_listen`]。`window` 為 0 回空集合且不開 socket。
pub fn passive_observe(
    network: &Ipv4Net,
    window: Duration,
) -> Result<Vec<(Ipv4Addr, Mac)>, ProbeError> {
    if window.is_zero() {
        return Ok(Vec::new());
    }

    let started = std::time::Instant::now();
    let mut senders = Vec::new();
    passive_listen(
        network,
        || started.elapsed() >= window,
        |address, mac| {
            senders.push((address, mac));
        },
    )?;
    Ok(senders)
}

/// 啟動能力檢查：對網段介面開一次 raw socket 後立即關閉。
///
/// 供啟動時提示（缺 `CAP_NET_RAW` 時主動掃描必然失敗）；不阻擋啟動。
#[cfg(target_os = "linux")]
pub fn raw_available(network: &Ipv4Net) -> Result<(), ProbeError> {
    let (interface, _) = local_interface(network)
        .ok_or_else(|| ProbeError::Unusable(format!("找不到落在網段 {network} 內的本機介面")))?;
    open_arp_socket(&interface).map(|_| ())
}

/// 非 Linux：raw 一律不可用。
#[cfg(not(target_os = "linux"))]
pub fn raw_available(_network: &Ipv4Net) -> Result<(), ProbeError> {
    Err(ProbeError::Unusable(
        "非 Linux 不支援 raw ARP 探測".to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn parse_arp_sender_accepts_requests_and_replies() {
        let request = build_arp_request("0A:0B:0C:0D:0E:01", addr("10.0.0.2"), addr("10.0.0.7"))
            .expect("合法 MAC");
        assert_eq!(
            parse_arp_sender(&request),
            Some((addr("10.0.0.2"), "0a:0b:0c:0d:0e:01".to_string())),
            "request（opcode 1）的 sender 是有效證據"
        );

        let reply = reply_frame(
            "0A:0B:0C:0D:0E:02",
            addr("10.0.0.7"),
            "aa:bb:cc:dd:ee:01",
            addr("10.0.0.2"),
        );
        assert_eq!(
            parse_arp_sender(&reply),
            Some((addr("10.0.0.7"), "0a:0b:0c:0d:0e:02".to_string())),
            "reply（opcode 2）的 sender 解析相同"
        );
    }

    #[test]
    fn parse_arp_sender_rejects_foreign_and_malformed_frames() {
        let request = build_arp_request("aa:bb:cc:dd:ee:01", addr("10.0.0.2"), addr("10.0.0.7"))
            .expect("合法 MAC");

        assert_eq!(parse_arp_sender(&request[..41]), None, "長度不足");
        assert_eq!(parse_arp_sender(&[]), None);

        let mut non_arp = request.clone();
        non_arp[12..14].copy_from_slice(&0x0800u16.to_be_bytes());
        assert_eq!(parse_arp_sender(&non_arp), None, "非 ARP EtherType");

        let mut bad_length = request.clone();
        bad_length[18] = 8; // hlen 非 6
        assert_eq!(parse_arp_sender(&bad_length), None, "硬體長度不符");

        let mut rarp = request.clone();
        rarp[20..22].copy_from_slice(&3u16.to_be_bytes()); // opcode 3（RARP request）
        assert_eq!(parse_arp_sender(&rarp), None, "只接受 opcode 1／2");
    }

    #[test]
    fn passive_observe_with_zero_window_never_touches_network() {
        let network: Ipv4Net = "10.0.0.0/24".parse().expect("合法網段");
        assert_eq!(
            passive_observe(&network, Duration::ZERO).expect("零窗不開 socket"),
            Vec::new()
        );
    }

    #[test]
    fn passive_observe_without_local_interface_fails_before_opening_socket() {
        // TEST-NET-1 保留位址，本機不可能有介面落在其中（同 config 測試手法）。
        let network: Ipv4Net = "192.0.2.0/24".parse().expect("合法網段");
        let error = passive_observe(&network, Duration::from_millis(1))
            .expect_err("無本機介面應失敗")
            .to_string();
        assert!(
            error.contains("192.0.2.0/24"),
            "錯誤訊息須說明網段：{error}"
        );
    }

    #[test]
    fn valid_passive_sender_excludes_invalid_multicast_broadcast_and_local() {
        let local_ip = addr("10.0.0.2");
        let local_mac = "aa:bb:cc:dd:ee:02";
        let broadcast = addr("10.0.0.255");

        assert!(
            valid_passive_sender(
                addr("10.0.9.9"),
                "aa:bb:cc:dd:ee:99",
                local_ip,
                local_mac,
                broadcast
            ),
            "一般 sender 合法"
        );
        assert!(
            !valid_passive_sender(
                addr("0.0.0.0"),
                "aa:bb:cc:dd:ee:99",
                local_ip,
                local_mac,
                broadcast
            ),
            "排除 0.0.0.0（ARP probe）"
        );
        assert!(
            !valid_passive_sender(
                addr("224.0.0.1"),
                "aa:bb:cc:dd:ee:99",
                local_ip,
                local_mac,
                broadcast
            ),
            "排除 multicast"
        );
        assert!(
            !valid_passive_sender(
                addr("255.255.255.255"),
                "aa:bb:cc:dd:ee:99",
                local_ip,
                local_mac,
                broadcast
            ),
            "排除有限廣播"
        );
        assert!(
            !valid_passive_sender(
                broadcast,
                "aa:bb:cc:dd:ee:99",
                local_ip,
                local_mac,
                broadcast
            ),
            "排除網段廣播"
        );
        assert!(
            !valid_passive_sender(
                local_ip,
                "aa:bb:cc:dd:ee:99",
                local_ip,
                local_mac,
                broadcast
            ),
            "排除本機位址"
        );
        assert!(
            !valid_passive_sender(
                addr("10.0.9.9"),
                "AA:BB:CC:DD:EE:02",
                local_ip,
                local_mac,
                broadcast
            ),
            "排除本機 MAC（不分大小寫）"
        );
    }

    #[test]
    fn reply_window_exits_when_all_answered_idle_or_window_reached() {
        let window = Duration::from_secs(2);
        let idle = Duration::from_millis(500);
        let ms = Duration::from_millis;

        // 全部目標已回應 → 立即結束。
        assert!(reply_window_done(ms(0), ms(0), 3, 3, window, idle));
        assert!(!reply_window_done(ms(0), ms(0), 2, 3, window, idle));
        // 距最後一幀達 idle → 提早結束（含自始無幀）。
        assert!(reply_window_done(ms(500), ms(500), 1, 3, window, idle));
        assert!(!reply_window_done(ms(499), ms(499), 1, 3, window, idle));
        // 有新幀持續進來：未達 idle、未全回應、未達硬上限 → 繼續。
        assert!(!reply_window_done(ms(700), ms(200), 1, 3, window, idle));
        // 硬上限：即使幀持續進來也結束。
        assert!(reply_window_done(window, ms(10), 0, 3, window, idle));
        assert!(!reply_window_done(ms(1_999), ms(10), 0, 3, window, idle));
    }

    #[test]
    fn batch_pacing_delay_only_sleeps_remainder_of_interval() {
        let interval = Duration::from_secs(1);
        assert_eq!(
            batch_pacing_delay(Duration::ZERO, interval),
            interval,
            "未耗時：補足完整間隔"
        );
        assert_eq!(
            batch_pacing_delay(Duration::from_millis(800), interval),
            Duration::from_millis(200),
            "耗時 800ms：只補 200ms"
        );
        assert_eq!(
            batch_pacing_delay(interval, interval),
            Duration::ZERO,
            "耗時恰達間隔：不再等待"
        );
        assert_eq!(
            batch_pacing_delay(Duration::from_millis(1_500), interval),
            Duration::ZERO,
            "耗時超過間隔：不再等待"
        );
    }

    #[test]
    fn probe_targets_with_empty_targets_never_touches_network() {
        let network: Ipv4Net = "10.0.0.0/24".parse().expect("合法網段");
        assert_eq!(
            probe_targets(&network, &[], 1_000).expect("空目標不應開 socket"),
            SweepResult::default()
        );
    }
}
