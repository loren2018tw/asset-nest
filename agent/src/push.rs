//! 觀測回報推送：報告切批、記憶體佇列、失敗退避與後端回應處理。
//!
//! sweep（主動掃描）與 passive（被動 flush）報告共用同一佇列與推送路徑
//! （見票 04、票 05）。端點 `POST {AGENT_SERVER_URL}/api/v1/agents/observations`
//! （標頭 `X-Auth-Code`；見 spec §推送）。後端回應語意（見
//! `backend/src/api/agents.rs`）：
//!
//! - 2xx＋`{"stored":true}`：入庫成功，退避歸零。
//! - 2xx＋`{"stored":false,"reason":"subnet_unmatched"}`：記一次警告並丟棄
//!   該批（避免無限重送）。
//! - 401：記明確錯誤，批次保留、依退避重試（不熱迴圈）。
//! - 連線錯誤／5xx：批次保留、依退避重試（5s→5min 指數、上限 5min）。
//!
//! 記憶體佇列上限 [`QUEUE_CAPACITY`] 筆（checked＋seen），滿載丟最舊並警告；
//! 每請求合計上限 [`MAX_REQUEST_ENTRIES`] 筆，超出拆成多筆請求。

use std::collections::VecDeque;
use std::net::Ipv4Addr;
use std::sync::Mutex;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::Serialize;
use tokio::sync::Notify;

use crate::probe::Mac;

/// 每請求的觀測筆數上限（`checked`＋`seen`；見 spec §推送）。
pub const MAX_REQUEST_ENTRIES: usize = 5_000;
/// 記憶體佇列的筆數上限（滿載丟最舊；見 spec §推送）。
pub const QUEUE_CAPACITY: usize = 10_000;
/// 失敗退避起點（5 秒）。
pub const BACKOFF_BASE: Duration = Duration::from_secs(5);
/// 失敗退避上限（5 分鐘）。
pub const BACKOFF_MAX: Duration = Duration::from_secs(300);

/// 觀測回報的認證碼標頭（與後端 `api::agents` 一致）。
const AUTH_CODE_HEADER: &str = "X-Auth-Code";
/// 觀測入庫端點路徑。
const OBSERVATIONS_PATH: &str = "/api/v1/agents/observations";

/// `seen` 的一筆：位址與正規化小寫冒號 MAC。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Seen {
    pub address: Ipv4Addr,
    pub mac: Mac,
}

/// 單筆 sweep 報告（見 spec §週期掃描）；`observed_at` 為
/// `YYYY-MM-DDTHH:MM:SSZ`（UTC）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SweepReport {
    pub observed_at: String,
    /// 全部送出的位址。
    pub checked: Vec<Ipv4Addr>,
    /// 有回應的位址與 MAC。
    pub seen: Vec<Seen>,
}

impl SweepReport {
    /// 以觀測時間與結果建立報告（`observed_at` 格式化為後端接受的 UTC 樣式）。
    pub fn new(observed_at: DateTime<Utc>, checked: Vec<Ipv4Addr>, seen: Vec<Seen>) -> Self {
        Self {
            observed_at: timestamp(observed_at),
            checked,
            seen,
        }
    }

    /// 本報告的觀測筆數（`checked`＋`seen`）。
    pub fn entries(&self) -> usize {
        self.checked.len() + self.seen.len()
    }
}

/// 單筆 passive 報告（見 spec §持續被動）；`observed_at` 為 flush 時間
/// （`YYYY-MM-DDTHH:MM:SSZ` UTC），`senders` 為自上輪 flush 後有更新者。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PassiveReport {
    pub observed_at: String,
    pub senders: Vec<Seen>,
}

impl PassiveReport {
    /// 以 flush 時間與 sender 清單建立報告。
    pub fn new(observed_at: DateTime<Utc>, senders: Vec<Seen>) -> Self {
        Self {
            observed_at: timestamp(observed_at),
            senders,
        }
    }

    /// 本報告的觀測筆數。
    pub fn entries(&self) -> usize {
        self.senders.len()
    }
}

/// 推送佇列的通用報告：sweep／passive 共用切批、佇列與失敗處理。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Report {
    Sweep(SweepReport),
    Passive(PassiveReport),
}

impl Report {
    /// 本報告的觀測筆數。
    pub fn entries(&self) -> usize {
        match self {
            Report::Sweep(report) => report.entries(),
            Report::Passive(report) => report.entries(),
        }
    }

    /// 本報告的觀測時間。
    pub fn observed_at(&self) -> &str {
        match self {
            Report::Sweep(report) => &report.observed_at,
            Report::Passive(report) => &report.observed_at,
        }
    }
}

impl From<SweepReport> for Report {
    fn from(report: SweepReport) -> Self {
        Report::Sweep(report)
    }
}

impl From<PassiveReport> for Report {
    fn from(report: PassiveReport) -> Self {
        Report::Passive(report)
    }
}

/// 後端時間格式：`YYYY-MM-DDTHH:MM:SSZ`（UTC；與資料庫一致）。
pub fn timestamp(now: DateTime<Utc>) -> String {
    now.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// 推送的代理身分（每次請求皆帶）。
#[derive(Debug, Clone)]
pub struct AgentInfo {
    pub instance_id: String,
    pub name: String,
    pub version: String,
    pub subnet_cidr: String,
}

/// 推送結果（後端回應分類）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PushOutcome {
    /// 已入庫。
    Stored,
    /// 後端回報 CIDR 未對應；該批應丟棄。
    SubnetUnmatched,
    /// 認證碼不符（401）；批次保留、依退避重試。
    Unauthorized,
    /// 連線錯誤或 5xx 等可重試失敗（附原因）。
    Retry(String),
}

/// `try_flush` 的結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FlushResult {
    /// 佇列為空。
    Empty,
    /// 批次入庫，已自佇列移除。
    Stored { entries: usize },
    /// 後端回報未對應，已自佇列丟棄。
    Dropped { entries: usize },
    /// 認證碼不符；批次保留、需退避。
    Unauthorized { entries: usize },
    /// 可重試失敗；批次保留、需退避。
    Retry { entries: usize, reason: String },
}

impl FlushResult {
    /// 是否需退避等待後再重試（401 與連線／5xx 失敗）。
    pub fn needs_backoff(&self) -> bool {
        matches!(
            self,
            FlushResult::Unauthorized { .. } | FlushResult::Retry { .. }
        )
    }
}

/// 失敗退避：指數成長（`base * 2^n`）並以 `max` 為上限；成功時 [`reset`]。
///
/// [`reset`]: Backoff::reset
#[derive(Debug, Clone)]
pub struct Backoff {
    base: Duration,
    max: Duration,
    failures: u32,
}

impl Backoff {
    /// 以指定起點與上限建立。
    pub fn new(base: Duration, max: Duration) -> Self {
        Self {
            base,
            max,
            failures: 0,
        }
    }

    /// 取下一個等待時間（5s、10s、20s、…、上限；見 spec §推送）。
    pub fn next_delay(&mut self) -> Duration {
        let factor = 1u32.checked_shl(self.failures.min(31)).unwrap_or(u32::MAX);
        let delay = self.base.saturating_mul(factor).min(self.max);
        self.failures = self.failures.saturating_add(1);
        delay
    }

    /// 成功後歸零。
    pub fn reset(&mut self) {
        self.failures = 0;
    }

    /// 累計失敗次數。
    pub fn failures(&self) -> u32 {
        self.failures
    }
}

impl Default for Backoff {
    fn default() -> Self {
        Self::new(BACKOFF_BASE, BACKOFF_MAX)
    }
}

/// 記憶體推送佇列：佇列元素為已切批的 [`Report`]（每筆 ≤
/// [`MAX_REQUEST_ENTRIES`]），總筆數上限 `capacity`，滿載自最舊丟棄。
pub struct PushQueue {
    capacity: usize,
    state: Mutex<QueueState>,
    notify: Notify,
}

/// [`PushQueue`] 的內部狀態。
#[derive(Debug, Default)]
struct QueueState {
    chunks: VecDeque<Report>,
    entries: usize,
}

impl PushQueue {
    /// 建立佇列（容量為筆數上限）。
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            state: Mutex::new(QueueState::default()),
            notify: Notify::new(),
        }
    }

    /// 佇列目前筆數。
    pub fn entries(&self) -> usize {
        self.state.lock().expect("推送佇列鎖").entries
    }

    /// 佇列是否為空。
    pub fn is_empty(&self) -> bool {
        self.entries() == 0
    }

    /// 將報告切批後入列；滿載時丟棄最舊的批次並記警告，回傳丟棄筆數。
    pub fn push(&self, report: impl Into<Report>) -> usize {
        let chunks = split_report(report, MAX_REQUEST_ENTRIES);
        let mut state = self.state.lock().expect("推送佇列鎖");
        let mut dropped = 0usize;

        for chunk in chunks {
            while state.entries + chunk.entries() > self.capacity && !state.chunks.is_empty() {
                let oldest = state.chunks.pop_front().expect("佇列非空");
                state.entries -= oldest.entries();
                dropped += oldest.entries();
            }
            state.entries += chunk.entries();
            state.chunks.push_back(chunk);
        }

        if dropped > 0 {
            tracing::warn!(
                capacity = self.capacity,
                dropped,
                "推送佇列已滿，丟棄最舊的觀測"
            );
        }
        dropped
    }

    /// 取出（不移除）前端批次，總筆數不超過 `max_entries`；佇列空回 `None`。
    pub fn peek(&self, max_entries: usize) -> Option<Vec<Report>> {
        let state = self.state.lock().expect("推送佇列鎖");
        if state.chunks.is_empty() {
            return None;
        }

        let limit = max_entries.max(1);
        let mut batch = Vec::new();
        let mut total = 0usize;
        for chunk in &state.chunks {
            let entries = chunk.entries();
            if !batch.is_empty() && total + entries > limit {
                break;
            }
            batch.push(chunk.clone());
            total += entries;
        }
        Some(batch)
    }

    /// 移除 [`peek`](Self::peek) 回傳的前 `chunks` 個批次。
    pub fn commit(&self, chunks: usize) {
        let mut state = self.state.lock().expect("推送佇列鎖");
        for _ in 0..chunks {
            if let Some(chunk) = state.chunks.pop_front() {
                state.entries -= chunk.entries();
            }
        }
    }

    /// 喚醒等待中的推送迴圈（有新資料時呼叫）。
    pub fn notify(&self) {
        self.notify.notify_one();
    }

    /// 等待新資料（推送迴圈在佇列空時等待）。
    pub async fn notified(&self) {
        self.notify.notified().await;
    }
}

/// 將報告切為每筆不超過 `max_entries` 的多筆報告（純函式）。
///
/// `observed_at` 不變；空的報告回空集合。sweep 依 `checked`／`seen`、
/// passive 依 `senders` 切批。
pub fn split_report(report: impl Into<Report>, max_entries: usize) -> Vec<Report> {
    let max = max_entries.max(1);
    match report.into() {
        Report::Sweep(sweep) => split_sweep_report(sweep, max)
            .into_iter()
            .map(Report::Sweep)
            .collect(),
        Report::Passive(passive) => split_passive_report(passive, max)
            .into_iter()
            .map(Report::Passive)
            .collect(),
    }
}

/// 切分 sweep 報告（`checked` 先於 `seen`；見 [`split_report`]）。
fn split_sweep_report(report: SweepReport, max: usize) -> Vec<SweepReport> {
    if report.entries() == 0 {
        return Vec::new();
    }
    if report.entries() <= max {
        return vec![report];
    }

    let SweepReport {
        observed_at,
        checked,
        seen,
    } = report;
    let mut chunks = Vec::new();
    let mut current_checked: Vec<Ipv4Addr> = Vec::new();
    let mut current_seen: Vec<Seen> = Vec::new();
    let mut count = 0usize;
    let mut flush = |checked: &mut Vec<Ipv4Addr>, seen: &mut Vec<Seen>| {
        chunks.push(SweepReport {
            observed_at: observed_at.clone(),
            checked: std::mem::take(checked),
            seen: std::mem::take(seen),
        });
    };

    for address in checked {
        current_checked.push(address);
        count += 1;
        if count >= max {
            flush(&mut current_checked, &mut current_seen);
            count = 0;
        }
    }
    for entry in seen {
        current_seen.push(entry);
        count += 1;
        if count >= max {
            flush(&mut current_checked, &mut current_seen);
            count = 0;
        }
    }
    if count > 0 {
        flush(&mut current_checked, &mut current_seen);
    }

    chunks
}

/// 切分 passive 報告（`senders` 依序切批；見 [`split_report`]）。
fn split_passive_report(report: PassiveReport, max: usize) -> Vec<PassiveReport> {
    if report.entries() == 0 {
        return Vec::new();
    }

    let PassiveReport {
        observed_at,
        senders,
    } = report;
    senders
        .chunks(max)
        .map(|chunk| PassiveReport {
            observed_at: observed_at.clone(),
            senders: chunk.to_vec(),
        })
        .collect()
}

/// 串接後端端點（`server_url` 不含尾端 `/`）。
pub fn endpoint(server_url: &str, path: &str) -> String {
    format!("{}{path}", server_url.trim_end_matches('/'))
}

/// 觀測回報推送器（每次請求帶 `X-Auth-Code` 與代理身分）。
pub struct Pusher {
    client: reqwest::Client,
    endpoint: String,
    auth_code: String,
    agent: AgentInfo,
}

impl Pusher {
    /// 建立推送器；`server_url` 須為合法 http(s) 網址。
    pub fn new(
        client: reqwest::Client,
        server_url: &str,
        auth_code: &str,
        agent: AgentInfo,
    ) -> anyhow::Result<Self> {
        reqwest::Url::parse(server_url).map_err(|error| {
            anyhow::anyhow!("AGENT_SERVER_URL 格式錯誤：{server_url}（{error}）")
        })?;
        Ok(Self {
            client,
            endpoint: endpoint(server_url, OBSERVATIONS_PATH),
            auth_code: auth_code.to_string(),
            agent,
        })
    }

    /// 送出一個請求（一批報告），回傳後端回應分類。
    pub async fn send(&self, reports: &[Report]) -> PushOutcome {
        let body = ObservationsBody {
            instance_id: &self.agent.instance_id,
            name: &self.agent.name,
            version: &self.agent.version,
            subnet_cidr: &self.agent.subnet_cidr,
            reports: reports.iter().map(report_body).collect(),
        };

        let response = match self
            .client
            .post(&self.endpoint)
            .header(AUTH_CODE_HEADER, &self.auth_code)
            .json(&body)
            .send()
            .await
        {
            Ok(response) => response,
            Err(error) => return PushOutcome::Retry(format!("連線失敗：{error}")),
        };

        let status = response.status();
        if status == reqwest::StatusCode::UNAUTHORIZED {
            return PushOutcome::Unauthorized;
        }
        if !status.is_success() {
            return PushOutcome::Retry(format!("後端回覆 HTTP {status}"));
        }

        match response.json::<ObservationsResponse>().await {
            Ok(parsed) if !parsed.stored => match parsed.reason.as_deref() {
                Some("subnet_unmatched") => PushOutcome::SubnetUnmatched,
                other => PushOutcome::Retry(format!("後端未入庫（reason={other:?}）")),
            },
            Ok(_) => PushOutcome::Stored,
            Err(error) => PushOutcome::Retry(format!("回應解析失敗：{error}")),
        }
    }
}

/// 觀測回報請求 body（見 spec §HTTP API）。
#[derive(Debug, Serialize)]
struct ObservationsBody<'a> {
    instance_id: &'a str,
    name: &'a str,
    version: &'a str,
    subnet_cidr: &'a str,
    reports: Vec<ReportBody<'a>>,
}

/// 請求 body 內的單筆報告：`kind` 決定適用欄位（見 spec §HTTP API）。
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum ReportBody<'a> {
    Sweep {
        observed_at: &'a str,
        checked: &'a [Ipv4Addr],
        seen: &'a [Seen],
    },
    Passive {
        observed_at: &'a str,
        senders: &'a [Seen],
    },
}

/// 將通用報告轉為請求 body 的一筆（保留 `kind` 與適用欄位）。
fn report_body(report: &Report) -> ReportBody<'_> {
    match report {
        Report::Sweep(report) => ReportBody::Sweep {
            observed_at: &report.observed_at,
            checked: &report.checked,
            seen: &report.seen,
        },
        Report::Passive(report) => ReportBody::Passive {
            observed_at: &report.observed_at,
            senders: &report.senders,
        },
    }
}

/// 觀測回報回應（見 `backend/src/api/agents.rs`）。
#[derive(Debug, serde::Deserialize)]
struct ObservationsResponse {
    #[serde(default)]
    stored: bool,
    #[serde(default)]
    reason: Option<String>,
}

/// 嘗試推送佇列最前批次一次（見 spec §推送）。
///
/// 成功或 `subnet_unmatched` 時自佇列移除；401 與可重試失敗保留批次，
/// 由呼叫端依 [`Backoff`] 退避後再試。
pub async fn try_flush(pusher: &Pusher, queue: &PushQueue) -> FlushResult {
    let Some(batch) = queue.peek(MAX_REQUEST_ENTRIES) else {
        return FlushResult::Empty;
    };
    let entries: usize = batch.iter().map(Report::entries).sum();

    match pusher.send(&batch).await {
        PushOutcome::Stored => {
            queue.commit(batch.len());
            FlushResult::Stored { entries }
        }
        PushOutcome::SubnetUnmatched => {
            tracing::warn!(
                entries,
                "後端回報網段未對應（subnet_unmatched），丟棄本批觀測；請確認後端已建立對應網段"
            );
            queue.commit(batch.len());
            FlushResult::Dropped { entries }
        }
        PushOutcome::Unauthorized => {
            tracing::error!(
                entries,
                "後端拒絕代理認證碼（401）；請確認 AGENT_AUTH_CODE 與後端一致，將依退避重試"
            );
            FlushResult::Unauthorized { entries }
        }
        PushOutcome::Retry(reason) => FlushResult::Retry { entries, reason },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 測試用位址（由數值產生）。
    fn address(value: u32) -> Ipv4Addr {
        Ipv4Addr::from(value)
    }

    /// 測試用報告：`checked` 為連續位址、`seen` 為指定筆數。
    fn report(checked: u32, seen: usize) -> SweepReport {
        SweepReport {
            observed_at: "2026-10-06T12:00:00Z".to_string(),
            checked: (0..checked)
                .map(|offset| address(16_777_217 + offset))
                .collect(),
            seen: (0..seen)
                .map(|index| Seen {
                    address: address(16_777_217 + checked + index as u32),
                    mac: format!("aa:bb:cc:dd:ee:{index:02x}"),
                })
                .collect(),
        }
    }

    /// 測試用 passive 報告：`count` 筆 sender。
    fn passive(count: usize) -> PassiveReport {
        PassiveReport {
            observed_at: "2026-10-06T12:00:00Z".to_string(),
            senders: (0..count)
                .map(|index| Seen {
                    address: address(16_777_217 + index as u32),
                    mac: format!("aa:bb:cc:dd:ee:{index:02x}"),
                })
                .collect(),
        }
    }

    /// 取出切批結果中的 sweep 報告（測試斷言用）。
    fn sweeps(chunks: &[Report]) -> Vec<SweepReport> {
        chunks
            .iter()
            .filter_map(|chunk| match chunk {
                Report::Sweep(report) => Some(report.clone()),
                Report::Passive(_) => None,
            })
            .collect()
    }

    #[test]
    fn spec_limits_are_5000_and_10000() {
        assert_eq!(MAX_REQUEST_ENTRIES, 5_000);
        assert_eq!(QUEUE_CAPACITY, 10_000);
        assert_eq!(BACKOFF_BASE, Duration::from_secs(5));
        assert_eq!(BACKOFF_MAX, Duration::from_secs(300));
    }

    #[test]
    fn split_report_splits_at_max_entries_and_preserves_entries() {
        let original = report(3, 2);
        let chunks = split_report(original.clone(), 2);
        assert_eq!(chunks.len(), 3, "5 筆切為 2＋2＋1");
        assert!(
            chunks.iter().all(|chunk| chunk.entries() <= 2),
            "每筆不超過上限：{chunks:?}"
        );
        assert!(
            chunks
                .iter()
                .all(|chunk| chunk.observed_at() == original.observed_at.as_str()),
            "切批保留同一観測時間"
        );

        let chunks = sweeps(&chunks);
        let checked: Vec<Ipv4Addr> = chunks
            .iter()
            .flat_map(|chunk| chunk.checked.clone())
            .collect();
        let seen: Vec<Seen> = chunks.iter().flat_map(|chunk| chunk.seen.clone()).collect();
        assert_eq!(checked, original.checked, "checked 不重不漏");
        assert_eq!(seen, original.seen, "seen 不重不漏");
    }

    #[test]
    fn split_report_keeps_small_report_intact_and_drops_empty() {
        let small = report(1, 1);
        assert_eq!(
            split_report(small.clone(), 5_000),
            vec![Report::Sweep(small)]
        );
        assert!(split_report(report(0, 0), 5_000).is_empty(), "空報告不入列");
    }

    #[test]
    fn split_passive_report_splits_senders_and_preserves_entries() {
        let original = passive(5);
        let chunks = split_report(original.clone(), 2);
        assert_eq!(chunks.len(), 3, "5 筆切為 2＋2＋1");
        assert!(
            chunks.iter().all(|chunk| chunk.entries() <= 2),
            "每筆不超過上限：{chunks:?}"
        );

        let senders: Vec<Seen> = chunks
            .iter()
            .flat_map(|chunk| match chunk {
                Report::Passive(report) => report.senders.clone(),
                Report::Sweep(_) => Vec::new(),
            })
            .collect();
        assert_eq!(senders, original.senders, "senders 不重不漏");
        assert!(
            chunks
                .iter()
                .all(|chunk| chunk.observed_at() == original.observed_at.as_str()),
            "切批保留同一観測時間"
        );

        assert!(
            split_report(passive(0), 5_000).is_empty(),
            "空 passive 報告不入列"
        );
    }

    #[test]
    fn queue_drops_oldest_when_full() {
        let queue = PushQueue::new(3);
        assert_eq!(queue.push(report(2, 0)), 0, "首次入列不丟棄");
        assert_eq!(queue.entries(), 2);

        // 新報告 2 筆使總數超過 3：丟棄最舊的 2 筆。
        assert_eq!(queue.push(report(2, 0)), 2, "滿載丟棄最舊");
        assert_eq!(queue.entries(), 2);

        let batch = queue.peek(MAX_REQUEST_ENTRIES).expect("佇列非空");
        let checked: Vec<Ipv4Addr> = sweeps(&batch)
            .iter()
            .flat_map(|chunk| chunk.checked.clone())
            .collect();
        assert_eq!(
            checked,
            vec![address(16_777_217), address(16_777_218)],
            "保留的是較新的報告"
        );
    }

    #[test]
    fn queue_accepts_passive_reports_and_counts_senders() {
        let queue = PushQueue::new(QUEUE_CAPACITY);
        assert_eq!(queue.push(passive(2)), 0);
        assert_eq!(queue.entries(), 2);

        let batch = queue.peek(MAX_REQUEST_ENTRIES).expect("佇列非空");
        assert_eq!(batch.len(), 1);
        assert!(
            matches!(&batch[0], Report::Passive(report) if report.senders.len() == 2),
            "佇列元素為 passive 報告：{batch:?}"
        );
    }

    #[test]
    fn queue_peek_packs_chunks_up_to_request_limit() {
        let queue = PushQueue::new(QUEUE_CAPACITY);
        queue.push(report(3, 0));
        queue.push(report(3, 0));

        let packing = queue.peek(5).expect("佇列非空");
        assert_eq!(packing.len(), 1, "第二筆加入會超過上限 5");
        assert_eq!(packing.iter().map(Report::entries).sum::<usize>(), 3);

        let full = queue.peek(6).expect("佇列非空");
        assert_eq!(full.len(), 2, "上限 6 可容納兩筆");
        assert_eq!(full.iter().map(Report::entries).sum::<usize>(), 6);
    }

    #[test]
    fn queue_commit_removes_sent_chunks_in_order() {
        let queue = PushQueue::new(QUEUE_CAPACITY);
        queue.push(report(1, 0));
        queue.push(report(2, 0));

        let batch = queue.peek(2).expect("佇列非空");
        assert_eq!(batch.len(), 1, "第二筆加入會超過上限 2");
        assert_eq!(batch[0].entries(), 1);
        queue.commit(batch.len());

        assert_eq!(queue.entries(), 2);
        let remaining = queue.peek(MAX_REQUEST_ENTRIES).expect("仍有一批");
        assert_eq!(remaining[0].entries(), 2, "剩餘為第二筆");
    }

    #[test]
    fn backoff_grows_exponentially_and_caps() {
        let mut backoff = Backoff::new(Duration::from_millis(100), Duration::from_secs(1));
        let delays: Vec<Duration> = (0..6).map(|_| backoff.next_delay()).collect();
        assert_eq!(
            delays,
            vec![
                Duration::from_millis(100),
                Duration::from_millis(200),
                Duration::from_millis(400),
                Duration::from_millis(800),
                Duration::from_secs(1),
                Duration::from_secs(1),
            ],
            "指數成長至上限後維持"
        );

        backoff.reset();
        assert_eq!(backoff.failures(), 0);
        assert_eq!(
            backoff.next_delay(),
            Duration::from_millis(100),
            "重設後回到起點"
        );
    }

    #[test]
    fn backoff_default_is_five_seconds_to_five_minutes() {
        let mut backoff = Backoff::default();
        let delays: Vec<u64> = (0..8).map(|_| backoff.next_delay().as_secs()).collect();
        assert_eq!(delays, vec![5, 10, 20, 40, 80, 160, 300, 300]);
    }

    #[test]
    fn flush_result_needs_backoff_only_for_retryable_failures() {
        assert!(FlushResult::Unauthorized { entries: 1 }.needs_backoff());
        assert!(
            FlushResult::Retry {
                entries: 1,
                reason: "boom".to_string()
            }
            .needs_backoff()
        );
        assert!(!FlushResult::Empty.needs_backoff());
        assert!(!FlushResult::Stored { entries: 1 }.needs_backoff());
        assert!(!FlushResult::Dropped { entries: 1 }.needs_backoff());
    }

    #[test]
    fn sweep_report_new_formats_timestamp_and_counts_entries() {
        let observed_at = chrono::TimeZone::with_ymd_and_hms(&Utc, 2026, 10, 6, 8, 3, 2)
            .single()
            .expect("合法時間");
        let report = SweepReport::new(observed_at, vec![address(1)], vec![]);
        assert_eq!(report.observed_at, "2026-10-06T08:03:02Z");
        assert_eq!(report.entries(), 1);
    }

    #[test]
    fn passive_report_new_formats_flush_timestamp_and_counts_entries() {
        let observed_at = chrono::TimeZone::with_ymd_and_hms(&Utc, 2026, 10, 6, 8, 3, 2)
            .single()
            .expect("合法時間");
        let report = PassiveReport::new(
            observed_at,
            vec![Seen {
                address: address(1),
                mac: "aa:bb:cc:dd:ee:01".to_string(),
            }],
        );
        assert_eq!(
            report.observed_at, "2026-10-06T08:03:02Z",
            "flush 時間為觀測時間"
        );
        assert_eq!(report.entries(), 1);
    }
}
