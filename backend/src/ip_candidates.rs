//! 指派候選（IP candidates）領域模組：跨網段前綴搜尋可用位址與查詢位址狀態。
//!
//! 契約見 `.scratch/subnet-exclusions/spec.md` §8、ADR-0020；「可用」＝ v4 host、
//! 未指派、非池內、非排除範圍。前綴以 `.` 分段、octet 邊界：完整 octet 精確
//! 比對，最後一段一律為前綴段（比對十進位字串前綴；四段亦然，`10.1.1.6` 命中
//! `10.1.1.6` 與 `10.1.1.60–69`）；結尾帶點表示所有段皆為完整 octet（精確）。
//! 至少 2 個完整 octet，否則 400。匹配空間 ≤ /16（65,536 個位址）；有前綴段時
//! 依前綴段值輪流取樣（每值先取一筆、再回到首值），使各值皆出現在 `limit`
//! 視窗內；達 `limit` 即停止，不做無界掃描（見 spec §8 效能界線）。

use std::collections::HashMap;
use std::net::Ipv4Addr;

use ipnet::{IpNet, Ipv4Net};
use serde::Serialize;
use serde_json::json;
use sqlx::SqlitePool;

use crate::api::ApiError;
use crate::assignments;
use crate::ips::HostRange;
use crate::subnets::{self, Subnet};

/// 指派候選（可用位址；見 spec §8）。
#[derive(Debug, Serialize)]
pub struct Candidate {
    pub address: String,
    pub subnet_id: i64,
    pub subnet_cidr: String,
    pub subnet_name: Option<String>,
}

/// 查詢位址的狀態；`q` 為完整 v4 位址時才有值（見 spec §8）。
#[derive(Debug, Serialize)]
pub struct QueryStatus {
    pub address: String,
    /// `available`／`in_pool`／`excluded`／`static`／`reservation`／`out_of_subnet`。
    pub status: &'static str,
    pub subnet_id: Option<i64>,
    pub subnet_cidr: Option<String>,
    pub subnet_name: Option<String>,
}

/// 指派候選回應；無 `total`，`query_status` 非完整位址時為 `null`（見 spec §8）。
#[derive(Debug, Serialize)]
pub struct Candidates {
    pub items: Vec<Candidate>,
    pub query_status: Option<QueryStatus>,
}

/// 跨網段搜尋可用位址與查詢位址狀態（見 spec §8、票 04）。
///
/// `items`：以完整 octet 固定前段、前綴段展開為值集合 M、其後自由 octet；
/// 有前綴段時以值輪流取樣（每值先取一筆、再回到首值），檢查所屬網段
/// （CIDR 索引）與可用性，命中即收至 `limit`。
/// `query_status`：`q` 為完整 v4 位址時依判定順序回傳（見 [`status_for`]）。
pub async fn find(db: &SqlitePool, q: &str, limit: usize) -> Result<Candidates, ApiError> {
    let prefix = Prefix::parse(q)?;

    // 預載全部網段（含 pools 與排除範圍；三筆查詢、無 N+1）。
    let subnets = subnets::list_full(db)
        .await
        .map_err(|error| ApiError::internal("讀取網段清單失敗", error))?;
    let mut index = V4Index::build(&subnets)?;

    let mut items = Vec::new();
    for address in prefix.addresses() {
        if items.len() >= limit {
            break;
        }
        let Some(position) = index.find(address) else {
            continue;
        };
        ensure_assignments(db, &mut index, position).await?;
        let entry = &index.entries[position];
        if is_unavailable(entry, address) {
            continue;
        }
        items.push(Candidate {
            address: address.to_string(),
            subnet_id: entry.id,
            subnet_cidr: entry.cidr.clone(),
            subnet_name: entry.name.clone(),
        });
    }

    let query_status = match complete_address(q, &prefix) {
        Some(address) => Some(status_for(db, &mut index, address).await?),
        None => None,
    };

    Ok(Candidates {
        items,
        query_status,
    })
}

/// 位址不可作為候選：非 host、已指派、池內或排除範圍內（見 spec §8）。
fn is_unavailable(entry: &IndexedSubnet, address: Ipv4Addr) -> bool {
    !entry.hosts.contains(address)
        || entry
            .assignments
            .as_ref()
            .is_some_and(|cache| cache.contains_key(&u32::from(address)))
        || in_ranges(&entry.pools, address)
        || in_ranges(&entry.exclusions, address)
}

/// 完整位址的狀態；判定順序見 spec §8（無所屬網段 → 指派用途 → 非 host →
/// 池內 → 排除範圍 → 可用）。`subnet_*` 僅在落在某網段 CIDR 內時帶值。
async fn status_for(
    db: &SqlitePool,
    index: &mut V4Index,
    address: Ipv4Addr,
) -> Result<QueryStatus, ApiError> {
    let Some(position) = index.find(address) else {
        return Ok(QueryStatus {
            address: address.to_string(),
            status: "out_of_subnet",
            subnet_id: None,
            subnet_cidr: None,
            subnet_name: None,
        });
    };

    ensure_assignments(db, index, position).await?;
    let entry = &index.entries[position];

    let status = match entry
        .assignments
        .as_ref()
        .and_then(|cache| cache.get(&u32::from(address)))
    {
        Some(purpose) => purpose_status(purpose),
        None if !entry.hosts.contains(address) => "out_of_subnet",
        None if in_ranges(&entry.pools, address) => "in_pool",
        None if in_ranges(&entry.exclusions, address) => "excluded",
        None => "available",
    };

    Ok(QueryStatus {
        address: address.to_string(),
        status,
        subnet_id: Some(entry.id),
        subnet_cidr: Some(entry.cidr.clone()),
        subnet_name: entry.name.clone(),
    })
}

/// 解析後的查詢前綴（見 spec §8）。
#[derive(Debug, PartialEq, Eq)]
struct Prefix {
    /// 完整 octet（2–3 個；結尾帶點時 2–4 個）；固定位址前段、精確比對。
    complete: Vec<u8>,
    /// 前綴段展開的 octet 值集合（升冪）；結尾帶點時為 `None`（無前綴段）。
    partial_values: Option<Vec<u8>>,
}

impl Prefix {
    /// 解析查詢字串；前後空白忽略。至少 2 個完整 octet，格式錯誤回 400。
    /// 無結尾點時最後一段一律為前綴段（四段亦然）。
    fn parse(q: &str) -> Result<Self, ApiError> {
        let text = q.trim();
        if text.is_empty() {
            return Err(too_short_error());
        }

        // 結尾帶點＝所有段皆為完整 octet；split 產生的尾端空段即此點。
        let trailing_dot = text.ends_with('.');
        let mut segments: Vec<&str> = text.split('.').collect();
        if trailing_dot {
            segments.pop();
        }

        // 超過 4 段、空段（開頭點、連續點）、非數字或單段超過 3 位數皆為格式錯誤。
        if segments.len() > 4
            || segments.iter().any(|segment| {
                segment.is_empty()
                    || segment.len() > 3
                    || !segment.bytes().all(|b| b.is_ascii_digit())
            })
        {
            return Err(invalid_query_error(q));
        }

        // 僅結尾帶點視為所有段皆完整（items 精確）；否則最後一段為前綴段
        // （四段亦然：`10.1.1.6` 命中 `10.1.1.6` 與 `10.1.1.60–69`）。
        let complete_count = if trailing_dot {
            segments.len()
        } else {
            segments.len() - 1
        };
        if complete_count < 2 {
            return Err(too_short_error());
        }

        let mut complete = Vec::with_capacity(complete_count);
        for segment in &segments[..complete_count] {
            let value: u16 = segment.parse().map_err(|_| invalid_query_error(q))?;
            if value > u16::from(u8::MAX) {
                return Err(invalid_query_error(q));
            }
            complete.push(value as u8);
        }

        let partial_values = if trailing_dot {
            None
        } else {
            Some(prefix_values(segments[segments.len() - 1]))
        };

        Ok(Self {
            complete,
            partial_values,
        })
    }

    /// 產生匹配位址：有前綴段時依值輪流取樣（每值先取一筆、再回到首值）、
    /// 無前綴段時數值升冪；總數 ≤ 65,536（見 spec §8 效能界線）。
    fn addresses(&self) -> impl Iterator<Item = Ipv4Addr> + '_ {
        let octets = self.complete.len() as u32;
        let mut base = 0u32;
        for octet in &self.complete {
            base = (base << 8) | u32::from(*octet);
        }
        base <<= 8 * (4 - octets);

        // 前綴段佔第 octets+1 個 octet，其後為自由 octet（0..=255）。
        // 有前綴段時完整 octet 至多 3 個、無前綴段（結尾點）時 2–4 個；
        // 無前綴段以單一空值代入，輪流取樣退化為位址升冪。
        let values = self.partial_values.clone().unwrap_or_else(|| vec![0]);
        let free_bits = if self.partial_values.is_some() {
            8 * (3 - octets)
        } else {
            8 * (4 - octets)
        };
        let free_size = 1u32 << free_bits;
        let count = values.len() as u32;

        (0..count * free_size).map(move |index| {
            let partial_bits = u32::from(values[(index % count) as usize]) << free_bits;
            Ipv4Addr::from(base | partial_bits | (index / count))
        })
    }
}

/// 前綴段展開：十進位字串以 `partial` 為前綴的 octet 值（升冪）。
fn prefix_values(partial: &str) -> Vec<u8> {
    (0u16..=u16::from(u8::MAX))
        .filter(|value| value.to_string().starts_with(partial))
        .map(|value| value as u8)
        .collect()
}

/// `q` 為完整 v4 位址時回傳該位址；否則 `query_status` 為 `None`（見 spec §8）。
fn complete_address(q: &str, prefix: &Prefix) -> Option<Ipv4Addr> {
    let text = q.trim();
    if let Ok(address) = text.parse::<Ipv4Addr>() {
        // 僅接受正規寫法（避免 `010.0.0.1` 等等價寫法混同）。
        if address.to_string() == text {
            return Some(address);
        }
    }
    // 結尾帶點的四個完整 octet（如 `10.0.1.5.`）亦為完整位址。
    if prefix.partial_values.is_none() && prefix.complete.len() == 4 {
        return Some(Ipv4Addr::new(
            prefix.complete[0],
            prefix.complete[1],
            prefix.complete[2],
            prefix.complete[3],
        ));
    }
    None
}

/// 400 `invalid_query`：查詢前綴不足兩個完整 octet（field `q`）。
fn too_short_error() -> ApiError {
    ApiError::validation("查詢前綴至少須包含兩個完整 octet（例：10.0.）")
        .field("q")
        .detail("reason", json!("invalid_query"))
}

/// 400 `invalid_query`：非數字、超過 4 段、空段、完整 octet > 255（field `q`）。
fn invalid_query_error(q: &str) -> ApiError {
    ApiError::validation(format!("查詢前綴格式錯誤：{q}"))
        .field("q")
        .detail("reason", json!("invalid_query"))
}

/// v4 網段索引：依網路位址排序供二分搜尋；網段結構不重疊，至多命中一個。
struct V4Index {
    entries: Vec<IndexedSubnet>,
}

/// 單一 v4 網段的預解析內容；pool／排除範圍為數值區間，指派惰性快取。
struct IndexedSubnet {
    id: i64,
    cidr: String,
    name: Option<String>,
    network: Ipv4Net,
    hosts: HostRange,
    pools: Vec<(u32, u32)>,
    exclusions: Vec<(u32, u32)>,
    /// 指派位址 → 用途；`None`＝尚未讀取（逐網段快取，避免逐位址查詢）。
    assignments: Option<HashMap<u32, String>>,
}

impl V4Index {
    /// 由網段清單建立索引；v6 網段略過（pool 與排除範圍為 v4 限定）。
    fn build(subnets: &[Subnet]) -> Result<Self, ApiError> {
        let mut entries = Vec::new();
        for subnet in subnets {
            let network: IpNet = subnet
                .cidr
                .parse()
                .map_err(|error| ApiError::internal("網段 CIDR 格式錯誤", error))?;
            let IpNet::V4(network) = network else {
                continue;
            };

            let mut pools = Vec::with_capacity(subnet.pools.len());
            for pool in &subnet.pools {
                pools.push((
                    parse_stored_address(&pool.start_ip, "pool 位址格式錯誤")?,
                    parse_stored_address(&pool.end_ip, "pool 位址格式錯誤")?,
                ));
            }
            let mut exclusions = Vec::with_capacity(subnet.exclusions.len());
            for exclusion in &subnet.exclusions {
                exclusions.push((
                    parse_stored_address(&exclusion.start_ip, "排除範圍位址格式錯誤")?,
                    parse_stored_address(&exclusion.end_ip, "排除範圍位址格式錯誤")?,
                ));
            }

            entries.push(IndexedSubnet {
                id: subnet.id,
                cidr: subnet.cidr.clone(),
                name: subnet.name.clone(),
                hosts: HostRange::of(&network),
                network,
                pools,
                exclusions,
                assignments: None,
            });
        }
        entries.sort_by_key(|entry| u32::from(entry.network.network()));
        Ok(Self { entries })
    }

    /// 位址所屬網段索引；無所屬回傳 `None`。
    fn find(&self, address: Ipv4Addr) -> Option<usize> {
        let value = u32::from(address);
        let index = self
            .entries
            .partition_point(|entry| u32::from(entry.network.network()) <= value)
            .checked_sub(1)?;
        self.entries[index]
            .network
            .contains(&address)
            .then_some(index)
    }
}

/// 確保該網段的指派快取已載入；每次查詢每網段至多一次
/// （`assignments::list_for_subnet`；見 spec §8 效能界線）。
async fn ensure_assignments(
    db: &SqlitePool,
    index: &mut V4Index,
    position: usize,
) -> Result<(), ApiError> {
    if index.entries[position].assignments.is_some() {
        return Ok(());
    }

    let subnet_id = index.entries[position].id;
    let listed = assignments::list_for_subnet(db, subnet_id)
        .await
        .map_err(|error| ApiError::internal("讀取指派清單失敗", error))?;

    let mut cache = HashMap::with_capacity(listed.len());
    for assignment in listed {
        let address: Ipv4Addr = assignment
            .address
            .parse()
            .map_err(|error| ApiError::internal("指派位址格式錯誤", error))?;
        cache.insert(u32::from(address), assignment.purpose);
    }
    index.entries[position].assignments = Some(cache);
    Ok(())
}

/// 位址是否落在任一（起、訖）區間（端點皆含）。
fn in_ranges(ranges: &[(u32, u32)], address: Ipv4Addr) -> bool {
    let value = u32::from(address);
    ranges
        .iter()
        .any(|(start, end)| *start <= value && value <= *end)
}

/// 解析資料庫中已驗證過的 v4 位址文字；異常視為內部錯誤。
fn parse_stored_address(text: &str, context: &'static str) -> Result<u32, ApiError> {
    text.parse::<Ipv4Addr>()
        .map(u32::from)
        .map_err(|error| ApiError::internal(context, error))
}

/// 指派用途轉狀態字串；非 static 一律 reservation（比照 `ips::status_of`）。
fn purpose_status(purpose: &str) -> &'static str {
    if purpose == "static" {
        "static"
    } else {
        "reservation"
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
    fn prefix_parses_complete_and_partial_octets() {
        // 結尾帶點：三段皆為完整 octet，第四段自由。
        let prefix = Prefix::parse("10.0.1.").expect("合法前綴");
        assert_eq!(prefix.complete, vec![10, 0, 1]);
        assert_eq!(prefix.partial_values, None);

        // 未打點：最後一段為前綴段（1 → 1、10–19、100–199）。
        let prefix = Prefix::parse("10.0.1").expect("合法前綴");
        assert_eq!(prefix.complete, vec![10, 0]);
        let values = prefix.partial_values.clone().expect("含前綴段");
        assert_eq!(values.len(), 111);
        for value in [1, 10, 19, 100, 199] {
            assert!(values.contains(&value), "{value} 應命中");
        }
        for value in [0, 2, 9, 20, 99, 200, 255] {
            assert!(!values.contains(&value), "{value} 不應命中");
        }

        // 前綴段 0 → 僅 0；20 → 20、200–209。
        assert_eq!(prefix_values("0"), vec![0]);
        assert_eq!(
            prefix_values("20"),
            vec![20, 200, 201, 202, 203, 204, 205, 206, 207, 208, 209]
        );

        // 完整位址寫法（無結尾點）：前三段完整、第四段為前綴段（5、50–59）。
        let prefix = Prefix::parse("10.0.1.5").expect("合法前綴");
        assert_eq!(prefix.complete, vec![10, 0, 1]);
        assert_eq!(
            prefix.partial_values,
            Some(vec![5, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59])
        );

        // 結尾帶點的四段＝完整位址（items 精確）。
        let prefix = Prefix::parse("10.0.1.5.").expect("合法前綴");
        assert_eq!(prefix.complete, vec![10, 0, 1, 5]);
        assert_eq!(prefix.partial_values, None);
    }

    #[test]
    fn addresses_span_prefix_space_in_round_robin_order() {
        // 10.0.：兩個完整 octet、後兩段自由 → 65,536 個位址（/16 界線）。
        let prefix = Prefix::parse("10.0.").expect("合法前綴");
        assert_eq!(prefix.addresses().count(), 65_536);
        assert_eq!(prefix.addresses().next(), Some(addr("10.0.0.0")));
        assert_eq!(prefix.addresses().last(), Some(addr("10.0.255.255")));

        // 10.0.1.：第三段精確 1、第四段自由 → 10.0.1.0/24，不含 10.0.10.x。
        let prefix = Prefix::parse("10.0.1.").expect("合法前綴");
        assert_eq!(prefix.addresses().count(), 256);
        assert_eq!(prefix.addresses().next(), Some(addr("10.0.1.0")));
        assert_eq!(prefix.addresses().last(), Some(addr("10.0.1.255")));
        assert!(
            !prefix
                .addresses()
                .any(|address| address == addr("10.0.10.5"))
        );

        // 10.0.1：第三段 ∈ {1, 10–19, 100–199}、第四段自由；
        // 前綴段值輪流取樣（每值先取一筆、再回到首值）。
        let prefix = Prefix::parse("10.0.1").expect("合法前綴");
        assert_eq!(prefix.addresses().count(), 111 * 256);
        assert_eq!(
            prefix.addresses().take(3).collect::<Vec<_>>(),
            vec![addr("10.0.1.0"), addr("10.0.10.0"), addr("10.0.11.0")],
        );
        let first_round: Vec<_> = prefix.addresses().take(112).collect();
        assert_eq!(first_round[110], addr("10.0.199.0"), "第一輪取完所有值");
        assert_eq!(first_round[111], addr("10.0.1.1"), "第二輪回到首值");
        assert!(
            prefix
                .addresses()
                .any(|address| address == addr("10.0.10.5"))
        );
        assert!(
            !prefix
                .addresses()
                .any(|address| address == addr("10.0.2.5"))
        );
        assert!(
            !prefix
                .addresses()
                .any(|address| address == addr("10.0.20.5"))
        );

        // 10.0.1.5：前三段完整、第四段前綴段（5、50–59），輪流取樣退化為升冪；
        // 10.0.1.5.（結尾帶點）＝精確單一位址。
        let prefix = Prefix::parse("10.0.1.5").expect("合法前綴");
        assert_eq!(
            prefix.addresses().collect::<Vec<_>>(),
            vec![
                addr("10.0.1.5"),
                addr("10.0.1.50"),
                addr("10.0.1.51"),
                addr("10.0.1.52"),
                addr("10.0.1.53"),
                addr("10.0.1.54"),
                addr("10.0.1.55"),
                addr("10.0.1.56"),
                addr("10.0.1.57"),
                addr("10.0.1.58"),
                addr("10.0.1.59"),
            ]
        );
        let exact = Prefix::parse("10.0.1.5.").expect("合法前綴");
        assert_eq!(
            exact.addresses().collect::<Vec<_>>(),
            vec![addr("10.0.1.5")]
        );
    }

    #[test]
    fn invalid_prefixes_are_rejected() {
        for q in [
            "",
            "10",
            "10.0",
            "abc",
            "10.0.1.2.3",
            "256.0.1",
            "10..0",
            ".10.0",
            "10.0.1.2.3.4",
            "10.0.1234",
        ] {
            assert!(Prefix::parse(q).is_err(), "{q} 應被阻擋");
        }

        let error = Prefix::parse("10").expect_err("不足兩個完整 octet");
        assert_eq!(error.field_name(), Some("q"));
        assert_eq!(
            error.message(),
            "查詢前綴至少須包含兩個完整 octet（例：10.0.）"
        );

        let error = Prefix::parse("10.0.1.2.3").expect_err("超過 4 段");
        assert_eq!(error.field_name(), Some("q"));
        assert!(
            error.message().contains("格式錯誤"),
            "訊息：{}",
            error.message()
        );
    }

    #[test]
    fn query_status_only_for_complete_addresses() {
        let exact = |q: &str| {
            let prefix = Prefix::parse(q).expect("合法前綴");
            complete_address(q, &prefix)
        };

        assert_eq!(exact("10.0.1.5"), Some(addr("10.0.1.5")));
        assert_eq!(exact("10.0.1.5."), Some(addr("10.0.1.5")));
        assert_eq!(exact("10.0.1."), None, "結尾帶點三段＝前綴搜尋");
        assert_eq!(exact("10.0.1"), None, "前綴段");
    }
}
