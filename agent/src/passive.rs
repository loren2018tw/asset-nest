//! 持續被動監聽的聚合與 flush（見票 05、spec §持續被動）。
//!
//! [`crate::probe::passive_listen`] 於背景執行緒持續收 ARP（只收不送），
//! 每筆合法 sender 寫入 [`Aggregator`]（位址→最新 MAC，重複只更新並標記
//! 「自上輪 flush 後有更新」）。每 [`FLUSH_INTERVAL`] 由 runner 取出有更新者，
//! 以 flush 時間為 `observed_at` 產生 [`PassiveReport`]，排入既有推送佇列
//! （切批與失敗退避見 [`crate::push`]）。
//!
//! 聚合只保留「地址→最新 MAC」：重複 sender 不重複輸出；無更新則不送。

use std::collections::{HashMap, HashSet};
use std::net::Ipv4Addr;
use std::time::Duration;

use chrono::{DateTime, Utc};

use crate::probe::Mac;
use crate::push::{PassiveReport, Seen};

/// 被動聚合 flush 間隔（30 秒；見 spec §持續被動）。
pub const FLUSH_INTERVAL: Duration = Duration::from_secs(30);

/// 被動 sender 的記憶體聚合。
///
/// `senders` 保存每個位址的最新 MAC；`updated` 保存自上輪 flush 後有觀測的
/// 位址。位址曾觀測過但本輪無新觀測時不輸出（見 [`flush`](Self::flush)）。
#[derive(Debug, Default)]
pub struct Aggregator {
    senders: HashMap<Ipv4Addr, Mac>,
    updated: HashSet<Ipv4Addr>,
}

impl Aggregator {
    /// 建立空聚合器。
    pub fn new() -> Self {
        Self::default()
    }

    /// 記錄一筆 sender：同一位址只更新為最新 MAC，並標記待送。
    ///
    /// `mac` 須為正規化小寫冒號格式（來自 [`crate::probe::parse_arp_sender`]）。
    pub fn observe(&mut self, address: Ipv4Addr, mac: Mac) {
        self.senders.insert(address, mac);
        self.updated.insert(address);
    }

    /// 取出自上輪 flush 後有更新的 sender（位址遞增），並清除更新標記。
    pub fn updated_senders(&mut self) -> Vec<(Ipv4Addr, Mac)> {
        let mut addresses: Vec<Ipv4Addr> = self.updated.drain().collect();
        addresses.sort_unstable();

        addresses
            .into_iter()
            .filter_map(|address| self.senders.get(&address).map(|mac| (address, mac.clone())))
            .collect()
    }

    /// 以 `observed_at`（flush 時間）產生 passive 報告；本輪無更新回 `None`。
    pub fn flush(&mut self, observed_at: DateTime<Utc>) -> Option<PassiveReport> {
        let senders: Vec<Seen> = self
            .updated_senders()
            .into_iter()
            .map(|(address, mac)| Seen { address, mac })
            .collect();

        if senders.is_empty() {
            return None;
        }
        Some(PassiveReport::new(observed_at, senders))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 測試用位址。
    fn addr(text: &str) -> Ipv4Addr {
        text.parse().expect("合法位址")
    }

    /// 測試用固定時間（自 12:00:00 起算的秒數）。
    fn at(offset_secs: u32) -> DateTime<Utc> {
        chrono::TimeZone::with_ymd_and_hms(
            &Utc,
            2026,
            10,
            6,
            12,
            offset_secs / 60,
            offset_secs % 60,
        )
        .single()
        .expect("合法時間")
    }

    /// 測試用 sender。
    fn seen(address: &str, mac: &str) -> Seen {
        Seen {
            address: addr(address),
            mac: mac.to_string(),
        }
    }

    #[test]
    fn flush_interval_is_thirty_seconds() {
        assert_eq!(FLUSH_INTERVAL, Duration::from_secs(30));
    }

    #[test]
    fn aggregator_dedupes_by_address_and_keeps_latest_mac() {
        let mut aggregator = Aggregator::new();
        aggregator.observe(addr("10.0.0.9"), "aa:bb:cc:dd:ee:01".to_string());
        aggregator.observe(addr("10.0.0.9"), "aa:bb:cc:dd:ee:02".to_string());
        aggregator.observe(addr("10.0.0.9"), "aa:bb:cc:dd:ee:02".to_string());

        let report = aggregator.flush(at(1)).expect("有更新");
        assert_eq!(
            report.senders,
            vec![seen("10.0.0.9", "aa:bb:cc:dd:ee:02")],
            "同一位址只輸出一次且為最新 MAC"
        );
    }

    #[test]
    fn flush_returns_only_updated_senders_and_clears_marks() {
        let mut aggregator = Aggregator::new();
        aggregator.observe(addr("10.0.0.10"), "aa:bb:cc:dd:ee:10".to_string());
        aggregator.observe(addr("10.0.0.9"), "aa:bb:cc:dd:ee:09".to_string());

        let first = aggregator.flush(at(30)).expect("首輪有更新");
        assert_eq!(
            first.senders,
            vec![
                seen("10.0.0.9", "aa:bb:cc:dd:ee:09"),
                seen("10.0.0.10", "aa:bb:cc:dd:ee:10"),
            ],
            "位址遞增輸出"
        );
        assert_eq!(first.observed_at, "2026-10-06T12:00:30Z");

        assert!(
            aggregator.flush(at(60)).is_none(),
            "無更新不產生報告（不送）"
        );

        aggregator.observe(addr("10.0.0.9"), "aa:bb:cc:dd:ee:99".to_string());
        let second = aggregator.flush(at(90)).expect("再次觀測算更新");
        assert_eq!(
            second.senders,
            vec![seen("10.0.0.9", "aa:bb:cc:dd:ee:99")],
            "只含有更新者，且為最新 MAC"
        );
        assert!(aggregator.flush(at(120)).is_none(), "flush 後標記已清除");
    }

    #[test]
    fn same_mac_reobserved_after_flush_counts_as_update() {
        let mut aggregator = Aggregator::new();
        aggregator.observe(addr("10.0.0.9"), "aa:bb:cc:dd:ee:09".to_string());
        assert!(aggregator.flush(at(30)).is_some());

        // 同一 MAC 再次出現仍算「自上輪 flush 後有更新」（最新證據）。
        aggregator.observe(addr("10.0.0.9"), "aa:bb:cc:dd:ee:09".to_string());
        let again = aggregator.flush(at(60)).expect("同 MAC 再觀測仍算更新");
        assert_eq!(again.senders, vec![seen("10.0.0.9", "aa:bb:cc:dd:ee:09")]);
    }
}
