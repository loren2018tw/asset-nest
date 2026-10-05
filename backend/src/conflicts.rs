//! 語意衝突（semantic conflict）領域模組：IpInPool、IpOutOfSubnet、
//! DuplicateHwAddress 的偵測、列標記與儲存警示；另提供 IP 清單列專用的
//! 觀測衍生衝突（ObservedMacMismatch、ObservedOnUnassigned；見 ADR-0014）。
//!
//! 詞彙依 `GLOSSARY.md`；規則見 `.scratch/asset-ip-management/spec.md` §3.2，
//! 決策見 ADR-0006：衝突是標記、不是狀態——僅提示、不阻擋儲存。
//! IpInUse 已由「同網段不重複指派」的結構規則涵蓋，不在此偵測。
//! 觀測碼即時計算、不落地，且不計入網段衝突數與指派儲存警示（見票 07）。

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr};

use ipnet::IpNet;

use crate::api::ApiError;
use crate::assignments::ListedAssignment;
use crate::interfaces::Warning;
use crate::observation::Presence;
use crate::subnets::Subnet;

/// IpInPool：指派位址落在該網段任一 DHCP 位址池內（僅 v4；pool 僅 v4 有）。
pub const IP_IN_POOL: &str = "IpInPool";
/// IpOutOfSubnet：指派位址不在該網段 CIDR 內（v4／v6 皆適用）。
pub const IP_OUT_OF_SUBNET: &str = "IpOutOfSubnet";
/// DuplicateHwAddress：同一網段同 MAC 出現多筆保留（purpose=reservation）。
pub const DUPLICATE_HW_ADDRESS: &str = "DuplicateHwAddress";
/// ObservedMacMismatch：已指派位址被觀測到由非宣告 MAC 使用（見 ADR-0014）。
pub const OBSERVED_MAC_MISMATCH: &str = "ObservedMacMismatch";
/// ObservedOnUnassigned：未指派且非池內位址被觀測到有主（見 ADR-0014）。
pub const OBSERVED_ON_UNASSIGNED: &str = "ObservedOnUnassigned";

/// 一筆指派命中的衝突；`codes` 僅含命中者，依固定順序排列
/// （IpOutOfSubnet、IpInPool、DuplicateHwAddress），確保列標記與警示穩定。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssignmentConflicts {
    pub address: String,
    pub codes: Vec<&'static str>,
}

/// 偵測某網段全部指派命中的語意衝突；僅回傳有命中者（依指派順序）。
///
/// 偵測一律即時計算、無快取：網段編輯（縮小 CIDR、擴大 pool）或取消指派後，
/// 下一次讀取即反映最新衝突（見票 07）。
pub fn detect(
    subnet: &Subnet,
    assignments: &[ListedAssignment],
) -> Result<Vec<AssignmentConflicts>, ApiError> {
    let network: IpNet = subnet
        .cidr
        .parse()
        .map_err(|error| ApiError::internal("網段 CIDR 格式錯誤", error))?;
    let pools = parse_pools(subnet)?;

    // DuplicateHwAddress：僅「保留」參與；同一 MAC 多筆保留時全部標記。
    // 同一 MAC 可能跨多介面（全系統 MAC 重複僅提示），故以 MAC 計數而非介面。
    let mut reservation_macs: HashMap<&str, usize> = HashMap::new();
    for assignment in assignments {
        if assignment.purpose == "reservation" {
            if let Some(mac) = assignment.mac.as_deref() {
                *reservation_macs.entry(mac).or_default() += 1;
            }
        }
    }

    let mut result = Vec::new();
    for assignment in assignments {
        let address: IpAddr = assignment
            .address
            .parse()
            .map_err(|error| ApiError::internal("指派位址格式錯誤", error))?;

        let mut codes = Vec::new();
        if !network.contains(&address) {
            codes.push(IP_OUT_OF_SUBNET);
        }
        if let IpAddr::V4(v4) = address {
            if pools.iter().any(|(start, end)| *start <= v4 && v4 <= *end) {
                codes.push(IP_IN_POOL);
            }
        }
        if assignment.purpose == "reservation"
            && assignment
                .mac
                .as_deref()
                .is_some_and(|mac| reservation_macs.get(mac).copied().unwrap_or(0) > 1)
        {
            codes.push(DUPLICATE_HW_ADDRESS);
        }

        if !codes.is_empty() {
            result.push(AssignmentConflicts {
                address: assignment.address.clone(),
                codes,
            });
        }
    }

    Ok(result)
}

/// 依位址索引衝突代碼；供 IP 清單列填入 `conflicts` 欄位。
pub fn by_address(
    subnet: &Subnet,
    assignments: &[ListedAssignment],
) -> Result<HashMap<String, Vec<&'static str>>, ApiError> {
    Ok(detect(subnet, assignments)?
        .into_iter()
        .map(|conflict| (conflict.address, conflict.codes))
        .collect())
}

/// 偵測某網段位址的觀測衍生衝突，以位址文字索引（即時計算、不落地；見票 07）。
///
/// 僅供 [`crate::ips::list`] 的列標記合併使用；[`detect`]（網段衝突數）與
/// [`warnings_for`]（指派儲存警示）不含觀測碼，語意分別維持不變。
///
/// 規則（`presence` 以位址文字索引）：
/// - [`OBSERVED_MAC_MISMATCH`]：位址已指派、宣告介面 MAC 非空，且現況
///   `last_seen_mac` 與其不同（不分大小寫）。
/// - [`OBSERVED_ON_UNASSIGNED`]：位址未指派、不在該網段任一 pool 內，
///   且現況 `last_seen_mac` 非空；池內位址可能由 DHCP 正常使用，不標記。
///
/// 回傳僅含命中者；呼叫端先放既有語意碼、再附加觀測碼（固定順序）。
pub fn observed_codes(
    subnet: &Subnet,
    assignments: &[ListedAssignment],
    presence: &HashMap<String, Presence>,
) -> Result<HashMap<String, Vec<&'static str>>, ApiError> {
    let pools = parse_pools(subnet)?;
    let mut by_address: HashMap<&str, &ListedAssignment> =
        HashMap::with_capacity(assignments.len());
    for assignment in assignments {
        by_address.insert(assignment.address.as_str(), assignment);
    }

    let mut result = HashMap::new();
    for (address, presence) in presence {
        let Some(seen_mac) = presence.last_seen_mac.as_deref() else {
            continue;
        };

        match by_address.get(address.as_str()) {
            Some(assignment) => {
                // 宣告介面無 MAC 時不比對（無從判定不符）。
                let Some(declared_mac) = assignment.mac.as_deref() else {
                    continue;
                };
                if !declared_mac.eq_ignore_ascii_case(seen_mac) {
                    result.insert(address.clone(), vec![OBSERVED_MAC_MISMATCH]);
                }
            }
            None => {
                let parsed: Ipv4Addr = address
                    .parse()
                    .map_err(|error| ApiError::internal("觀測現況位址格式錯誤", error))?;
                let in_pool = pools
                    .iter()
                    .any(|(start, end)| *start <= parsed && parsed <= *end);
                if !in_pool {
                    result.insert(address.clone(), vec![OBSERVED_ON_UNASSIGNED]);
                }
            }
        }
    }

    Ok(result)
}

/// 建立／更新指派後的警示訊息：該位址命中的衝突（僅提示、不阻擋儲存）。
///
/// 每次儲存都重新偵測整個網段：新增保留可能使既有保留也命中
/// DuplicateHwAddress；取消指派則使殘存保留的衝突自然消失。
pub fn warnings_for(
    subnet: &Subnet,
    assignments: &[ListedAssignment],
    address: &str,
) -> Result<Vec<Warning>, ApiError> {
    let conflicts = by_address(subnet, assignments)?;
    let Some(codes) = conflicts.get(address) else {
        return Ok(Vec::new());
    };

    let mac = assignments
        .iter()
        .find(|assignment| assignment.address == address)
        .and_then(|assignment| assignment.mac.as_deref());

    Ok(codes
        .iter()
        .map(|&code| Warning {
            code,
            message: message_of(code, address, &subnet.cidr, mac),
        })
        .collect())
}

/// 衝突代碼的中文說明（供儲存警示；前端徽章 tooltip 沿用同一組語意）。
fn message_of(code: &str, address: &str, cidr: &str, mac: Option<&str>) -> String {
    match code {
        IP_OUT_OF_SUBNET => {
            format!("位址 {address} 不在網段 {cidr} 內（僅提示，不阻擋儲存）")
        }
        IP_IN_POOL => {
            format!("位址 {address} 落在 DHCP 位址池內（僅提示，不阻擋儲存）")
        }
        DUPLICATE_HW_ADDRESS => format!(
            "MAC {} 在同一網段出現多筆保留（僅提示，不阻擋儲存）",
            mac.unwrap_or("（未知）")
        ),
        other => format!("語意衝突 {other}（僅提示，不阻擋儲存）"),
    }
}

/// 解析網段所有 pool 範圍（v4）；資料庫內容經結構驗證，格式異常視為內部錯誤。
fn parse_pools(subnet: &Subnet) -> Result<Vec<(Ipv4Addr, Ipv4Addr)>, ApiError> {
    subnet
        .pools
        .iter()
        .map(|pool| {
            let start = pool
                .start_ip
                .parse()
                .map_err(|error| ApiError::internal("pool 位址格式錯誤", error))?;
            let end = pool
                .end_ip
                .parse()
                .map_err(|error| ApiError::internal("pool 位址格式錯誤", error))?;
            Ok((start, end))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::subnets::Pool;

    /// 測試用網段；pools 以（起點、終點）表示。
    fn subnet(cidr: &str, pools: &[(&str, &str)]) -> Subnet {
        Subnet {
            id: 1,
            cidr: cidr.to_string(),
            name: None,
            note: None,
            gateway: None,
            kea_subnet_id: None,
            observed: false,
            discovery_enabled: false,
            discovery_interval_minutes: None,
            last_discovery_at: None,
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

    /// 測試用指派列；僅 address／purpose／mac 參與衝突偵測。
    fn listed(address: &str, purpose: &str, mac: Option<&str>) -> ListedAssignment {
        ListedAssignment {
            address: address.to_string(),
            purpose: purpose.to_string(),
            hostname: None,
            interface_id: 1,
            interface_name: Some("eth0".to_string()),
            mac: mac.map(str::to_string),
            asset_id: 1,
            asset_property_no: None,
            asset_description: "測試主機".to_string(),
            asset_brand: None,
            asset_model: None,
            asset_location: "機房 A".to_string(),
        }
    }

    #[test]
    fn ip_out_of_subnet_is_detected_for_v4_and_v6() {
        let v4 = subnet("10.0.0.0/25", &[]);
        let assignments = [
            listed("10.0.0.100", "static", None),
            listed("10.0.0.200", "static", None),
        ];
        let conflicts = detect(&v4, &assignments).expect("偵測成功");
        assert_eq!(
            conflicts,
            vec![AssignmentConflicts {
                address: "10.0.0.200".to_string(),
                codes: vec![IP_OUT_OF_SUBNET],
            }]
        );

        let v6 = subnet("fd00:0:0:1::/64", &[]);
        let assignments = [
            listed("fd00:0:0:1::5", "static", None),
            listed("fd00::5", "static", None),
        ];
        let conflicts = detect(&v6, &assignments).expect("偵測成功");
        assert_eq!(
            conflicts,
            vec![AssignmentConflicts {
                address: "fd00::5".to_string(),
                codes: vec![IP_OUT_OF_SUBNET],
            }]
        );
    }

    #[test]
    fn ip_in_pool_is_detected_only_for_v4() {
        let v4 = subnet("10.0.0.0/24", &[("10.0.0.10", "10.0.0.20")]);
        let assignments = [
            listed("10.0.0.10", "static", None),
            listed("10.0.0.21", "static", None),
        ];
        let conflicts = detect(&v4, &assignments).expect("偵測成功");
        assert_eq!(
            conflicts,
            vec![AssignmentConflicts {
                address: "10.0.0.10".to_string(),
                codes: vec![IP_IN_POOL],
            }]
        );
    }

    #[test]
    fn duplicate_hw_address_marks_every_duplicated_reservation() {
        let subnet = subnet("10.0.0.0/24", &[]);
        let assignments = [
            listed("10.0.0.1", "reservation", Some("aa:bb:cc:dd:ee:ff")),
            listed("10.0.0.2", "reservation", Some("aa:bb:cc:dd:ee:ff")),
            listed("10.0.0.3", "static", Some("aa:bb:cc:dd:ee:ff")),
            listed("10.0.0.4", "reservation", Some("11:22:33:44:55:66")),
        ];
        let conflicts = detect(&subnet, &assignments).expect("偵測成功");
        assert_eq!(
            conflicts,
            vec![
                AssignmentConflicts {
                    address: "10.0.0.1".to_string(),
                    codes: vec![DUPLICATE_HW_ADDRESS],
                },
                AssignmentConflicts {
                    address: "10.0.0.2".to_string(),
                    codes: vec![DUPLICATE_HW_ADDRESS],
                },
            ],
            "僅多筆保留標記；同 MAC 的手動設定與單筆保留不標記"
        );
    }

    #[test]
    fn multiple_rules_on_one_assignment_are_ordered() {
        let subnet = subnet("10.0.0.0/25", &[("10.0.0.10", "10.0.0.20")]);
        let assignments = [
            listed("10.0.0.200", "reservation", Some("aa:bb:cc:dd:ee:ff")),
            listed("10.0.0.201", "reservation", Some("aa:bb:cc:dd:ee:ff")),
        ];
        let conflicts = detect(&subnet, &assignments).expect("偵測成功");
        assert_eq!(
            conflicts[0].codes,
            vec![IP_OUT_OF_SUBNET, DUPLICATE_HW_ADDRESS],
            "出界與 MAC 重複可並存，依固定順序"
        );
    }

    #[test]
    fn observed_codes_flag_mismatch_and_unassigned_but_never_pool() {
        let subnet = subnet("10.0.0.0/24", &[("10.0.0.10", "10.0.0.20")]);
        let assignments = [
            listed("10.0.0.1", "static", Some("aa:bb:cc:dd:ee:01")),
            listed("10.0.0.2", "static", None),
        ];
        let presence: HashMap<String, Presence> = [
            ("10.0.0.1", Some("AA:BB:CC:DD:EE:FF")), // 宣告與觀測不符（大小寫不影響）
            ("10.0.0.2", Some("aa:bb:cc:dd:ee:02")), // 宣告介面無 MAC
            ("10.0.0.3", Some("aa:bb:cc:dd:ee:03")), // 未指派且非池內
            ("10.0.0.10", Some("aa:bb:cc:dd:ee:10")), // 池內
            ("10.0.0.4", None),                      // 僅 last_checked_at
        ]
        .into_iter()
        .map(|(address, mac)| {
            (
                address.to_string(),
                Presence {
                    last_seen_mac: mac.map(str::to_string),
                    ..Presence::default()
                },
            )
        })
        .collect();

        let codes = observed_codes(&subnet, &assignments, &presence).expect("偵測成功");
        assert_eq!(codes.get("10.0.0.1"), Some(&vec![OBSERVED_MAC_MISMATCH]));
        assert_eq!(codes.get("10.0.0.2"), None, "宣告 MAC 為空不比對");
        assert_eq!(codes.get("10.0.0.3"), Some(&vec![OBSERVED_ON_UNASSIGNED]));
        assert_eq!(codes.get("10.0.0.10"), None, "池內位址不標記");
        assert_eq!(codes.get("10.0.0.4"), None, "無 last_seen_mac 不標記");
    }

    #[test]
    fn by_address_and_warnings_use_canonical_messages() {
        let subnet = subnet("10.0.0.0/24", &[("10.0.0.10", "10.0.0.20")]);
        let assignments = [listed("10.0.0.10", "static", None)];

        let map = by_address(&subnet, &assignments).expect("偵測成功");
        assert_eq!(map.get("10.0.0.10"), Some(&vec![IP_IN_POOL]));
        assert_eq!(map.get("10.0.0.11"), None);

        let warnings = warnings_for(&subnet, &assignments, "10.0.0.10").expect("偵測成功");
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].code, IP_IN_POOL);
        assert!(warnings[0].message.contains("池內"));
        assert!(warnings[0].message.contains("不阻擋"));

        let none = warnings_for(&subnet, &assignments, "10.0.0.11").expect("偵測成功");
        assert!(none.is_empty(), "無衝突時無警示");
    }

    #[test]
    fn duplicate_hw_address_warning_names_the_mac() {
        let subnet = subnet("10.0.0.0/24", &[]);
        let assignments = [
            listed("10.0.0.1", "reservation", Some("aa:bb:cc:dd:ee:ff")),
            listed("10.0.0.2", "reservation", Some("aa:bb:cc:dd:ee:ff")),
        ];

        let warnings = warnings_for(&subnet, &assignments, "10.0.0.2").expect("偵測成功");
        assert_eq!(warnings[0].code, DUPLICATE_HW_ADDRESS);
        assert!(warnings[0].message.contains("aa:bb:cc:dd:ee:ff"));
    }
}
