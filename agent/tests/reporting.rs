//! 心跳與觀測推送的整合測試：以本機 stub HTTP server 驗證連線行為。
//!
//! stub 以 `TcpListener` 最小實作（讀請求、記錄、依序回應），不新增依賴；
//! **不得**在此檔發真實網路探測（也不呼叫 `probe` 的送收路徑）。

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use asset_nest_agent::heartbeat::Heartbeat;
use asset_nest_agent::passive::Aggregator;
use asset_nest_agent::push::{
    AgentInfo, Backoff, FlushResult, PushQueue, Pusher, QUEUE_CAPACITY, Seen, SweepReport,
    try_flush,
};

/// stub server 收到的請求。
#[derive(Debug, Clone)]
struct CapturedRequest {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Value,
}

impl CapturedRequest {
    /// 取標頭值（不分大小寫）。
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

/// stub server 的單一預設回應。
#[derive(Debug, Clone)]
struct StubResponse {
    status: u16,
    body: Value,
}

/// 最小 HTTP stub server：依序回應佇列；佇列空時回 200 `{"stored":true}`。
#[derive(Clone)]
struct StubServer {
    requests: Arc<Mutex<Vec<CapturedRequest>>>,
    responses: Arc<Mutex<VecDeque<StubResponse>>>,
    base_url: String,
}

impl StubServer {
    /// 啟動 stub 並回傳其基底網址（`http://127.0.0.1:<port>`）。
    async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("綁定 stub");
        let address = listener.local_addr().expect("stub 位址");
        let server = Self {
            requests: Arc::new(Mutex::new(Vec::new())),
            responses: Arc::new(Mutex::new(VecDeque::new())),
            base_url: format!("http://{address}"),
        };

        let accepting = server.clone();
        tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    break;
                };
                let serving = accepting.clone();
                tokio::spawn(async move { serving.serve(stream).await });
            }
        });

        server
    }

    /// 預先排入一次回應（依序取用）。
    fn enqueue(&self, status: u16, body: Value) {
        self.responses
            .lock()
            .expect("stub 鎖")
            .push_back(StubResponse { status, body });
    }

    /// 已收到的請求（依序）。
    fn requests(&self) -> Vec<CapturedRequest> {
        self.requests.lock().expect("stub 鎖").clone()
    }

    /// 已收到的請求數。
    fn request_count(&self) -> usize {
        self.requests.lock().expect("stub 鎖").len()
    }

    /// 服務單一連線：讀取完整請求、記錄、回應後關閉。
    async fn serve(self, mut stream: TcpStream) {
        let mut buffer = Vec::new();
        let mut chunk = [0u8; 4096];
        let Some(header_end) = (loop {
            let read = match stream.read(&mut chunk).await {
                Ok(0) => return,
                Ok(read) => read,
                Err(_) => return,
            };
            buffer.extend_from_slice(&chunk[..read]);
            if let Some(position) = find_subslice(&buffer, b"\r\n\r\n") {
                break Some(position + 4);
            }
        }) else {
            return;
        };

        let header_text = String::from_utf8_lossy(&buffer[..header_end]).to_string();
        let mut lines = header_text.split("\r\n");
        let request_line = lines.next().unwrap_or_default();
        let mut parts = request_line.split_whitespace();
        let method = parts.next().unwrap_or_default().to_string();
        let path = parts.next().unwrap_or_default().to_string();

        let mut headers = Vec::new();
        let mut content_length = 0usize;
        for line in lines {
            if let Some((name, value)) = line.split_once(':') {
                let name = name.trim().to_string();
                let value = value.trim().to_string();
                if name.eq_ignore_ascii_case("content-length") {
                    content_length = value.parse().unwrap_or(0);
                }
                headers.push((name, value));
            }
        }

        let mut body = buffer[header_end..].to_vec();
        while body.len() < content_length {
            let read = match stream.read(&mut chunk).await {
                Ok(0) => break,
                Ok(read) => read,
                Err(_) => break,
            };
            body.extend_from_slice(&chunk[..read]);
        }
        let body: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);

        self.requests
            .lock()
            .expect("stub 鎖")
            .push(CapturedRequest {
                method,
                path,
                headers,
                body,
            });

        let response = self
            .responses
            .lock()
            .expect("stub 鎖")
            .pop_front()
            .unwrap_or(StubResponse {
                status: 200,
                body: json!({"stored": true}),
            });
        let payload = response.body.to_string();
        let reply = format!(
            "HTTP/1.1 {} {}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            response.status,
            reason(response.status),
            payload.len(),
            payload
        );
        let _ = stream.write_all(reply.as_bytes()).await;
        let _ = stream.shutdown().await;
    }
}

/// HTTP 狀態原因詞（僅供 stub 回應列）。
fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        401 => "Unauthorized",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        _ => "Status",
    }
}

/// 在緩衝區中尋找子序列（標頭結束判定）。
fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// 測試用代理身分。
fn agent_info() -> AgentInfo {
    AgentInfo {
        instance_id: "11111111-1111-1111-1111-111111111111".to_string(),
        name: "test-agent".to_string(),
        version: "0.1.0".to_string(),
        subnet_cidr: "10.0.0.0/24".to_string(),
    }
}

/// 測試用 sweep 報告（兩筆 checked、一筆 seen）。
fn sample_report() -> SweepReport {
    SweepReport {
        observed_at: "2026-10-06T12:00:00Z".to_string(),
        checked: vec![
            "10.0.0.1".parse().expect("位址"),
            "10.0.0.2".parse().expect("位址"),
        ],
        seen: vec![Seen {
            address: "10.0.0.1".parse().expect("位址"),
            mac: "aa:bb:cc:dd:ee:01".to_string(),
        }],
    }
}

/// 測試用固定時間（斷言 flush 報告的 `observed_at`）。
fn fixed_time(second: u32) -> chrono::DateTime<chrono::Utc> {
    chrono::TimeZone::with_ymd_and_hms(&chrono::Utc, 2026, 10, 6, 8, 3, second)
        .single()
        .expect("合法時間")
}

/// 統計請求 body 中 `reports` 的合計筆數（checked＋seen）。
fn report_entries(body: &Value) -> usize {
    body["reports"]
        .as_array()
        .expect("reports 為陣列")
        .iter()
        .map(|report| {
            report["checked"].as_array().map_or(0, Vec::len)
                + report["seen"].as_array().map_or(0, Vec::len)
        })
        .sum()
}

#[tokio::test]
async fn heartbeat_posts_expected_body_and_headers() {
    let server = StubServer::start().await;
    server.enqueue(200, json!({"subnet_matched": true}));

    let heartbeat = Heartbeat::new(
        reqwest::Client::new(),
        &server.base_url,
        "secret-code",
        agent_info(),
    )
    .expect("建立心跳");

    assert!(
        heartbeat.send().await.expect("心跳成功"),
        "回應 subnet_matched=true"
    );

    let requests = server.requests();
    assert_eq!(requests.len(), 1, "送出一次心跳");
    assert_eq!(requests[0].method, "POST");
    assert_eq!(requests[0].path, "/api/v1/agents/heartbeat");
    assert_eq!(
        requests[0].header("x-auth-code"),
        Some("secret-code"),
        "帶認證碼標頭"
    );
    assert_eq!(
        requests[0].body["instance_id"],
        "11111111-1111-1111-1111-111111111111"
    );
    assert_eq!(requests[0].body["name"], "test-agent");
    assert_eq!(requests[0].body["version"], "0.1.0");
    assert_eq!(requests[0].body["subnet_cidr"], "10.0.0.0/24");
}

#[tokio::test]
async fn heartbeat_returns_false_when_subnet_unmatched() {
    let server = StubServer::start().await;
    server.enqueue(200, json!({"subnet_matched": false}));

    let heartbeat = Heartbeat::new(
        reqwest::Client::new(),
        &server.base_url,
        "secret-code",
        agent_info(),
    )
    .expect("建立心跳");

    assert!(
        !heartbeat.send().await.expect("心跳成功"),
        "subnet_matched=false 由呼叫端記一次警告"
    );
}

#[tokio::test]
async fn sweep_reports_are_pushed_with_header_and_shape() {
    let server = StubServer::start().await;
    let pusher = Pusher::new(
        reqwest::Client::new(),
        &server.base_url,
        "secret-code",
        agent_info(),
    )
    .expect("建立推送器");
    let queue = PushQueue::new(QUEUE_CAPACITY);
    queue.push(sample_report());

    let result = try_flush(&pusher, &queue).await;
    assert_eq!(result, FlushResult::Stored { entries: 3 });
    assert_eq!(queue.entries(), 0, "成功後批次自佇列移除");

    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "POST");
    assert_eq!(requests[0].path, "/api/v1/agents/observations");
    assert_eq!(requests[0].header("x-auth-code"), Some("secret-code"));

    let body = &requests[0].body;
    assert_eq!(body["instance_id"], "11111111-1111-1111-1111-111111111111");
    assert_eq!(body["name"], "test-agent");
    assert_eq!(body["version"], "0.1.0");
    assert_eq!(body["subnet_cidr"], "10.0.0.0/24");

    let reports = body["reports"].as_array().expect("reports 為陣列");
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0]["kind"], "sweep");
    assert_eq!(reports[0]["observed_at"], "2026-10-06T12:00:00Z");
    assert_eq!(
        reports[0]["checked"],
        json!(["10.0.0.1", "10.0.0.2"]),
        "checked 為送出的全部位址"
    );
    assert_eq!(reports[0]["seen"][0]["address"], "10.0.0.1");
    assert_eq!(reports[0]["seen"][0]["mac"], "aa:bb:cc:dd:ee:01");
}

#[tokio::test]
async fn passive_reports_are_pushed_with_header_and_shape() {
    let server = StubServer::start().await;
    let pusher = Pusher::new(
        reqwest::Client::new(),
        &server.base_url,
        "secret-code",
        agent_info(),
    )
    .expect("建立推送器");
    let queue = PushQueue::new(QUEUE_CAPACITY);

    let mut aggregator = Aggregator::new();
    aggregator.observe(
        "10.0.9.9".parse().expect("位址"),
        "aa:bb:cc:dd:ee:99".to_string(),
    );
    aggregator.observe(
        "10.0.9.10".parse().expect("位址"),
        "aa:bb:cc:dd:ee:10".to_string(),
    );
    let report = aggregator.flush(fixed_time(2)).expect("有更新");
    queue.push(report);

    let result = try_flush(&pusher, &queue).await;
    assert_eq!(result, FlushResult::Stored { entries: 2 });
    assert_eq!(queue.entries(), 0, "成功後批次自佇列移除");

    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "POST");
    assert_eq!(requests[0].path, "/api/v1/agents/observations");
    assert_eq!(requests[0].header("x-auth-code"), Some("secret-code"));

    let body = &requests[0].body;
    assert_eq!(body["instance_id"], "11111111-1111-1111-1111-111111111111");
    assert_eq!(body["subnet_cidr"], "10.0.0.0/24");

    let reports = body["reports"].as_array().expect("reports 為陣列");
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0]["kind"], "passive");
    assert_eq!(
        reports[0]["observed_at"], "2026-10-06T08:03:02Z",
        "以 flush 時間為觀測時間"
    );
    assert_eq!(
        reports[0]["senders"],
        json!([
            {"address": "10.0.9.9", "mac": "aa:bb:cc:dd:ee:99"},
            {"address": "10.0.9.10", "mac": "aa:bb:cc:dd:ee:10"},
        ])
    );
    assert!(
        reports[0].get("checked").is_none(),
        "passive 不帶 sweep 欄位"
    );
    assert!(reports[0].get("seen").is_none(), "passive 不帶 sweep 欄位");
}

#[tokio::test]
async fn passive_flush_pushes_only_senders_updated_since_last_flush() {
    let server = StubServer::start().await;
    let pusher = Pusher::new(
        reqwest::Client::new(),
        &server.base_url,
        "secret-code",
        agent_info(),
    )
    .expect("建立推送器");
    let queue = PushQueue::new(QUEUE_CAPACITY);

    let mut aggregator = Aggregator::new();
    aggregator.observe(
        "10.0.9.9".parse().expect("位址"),
        "aa:bb:cc:dd:ee:99".to_string(),
    );
    queue.push(aggregator.flush(fixed_time(2)).expect("首輪有更新"));
    assert!(
        aggregator.flush(fixed_time(3)).is_none(),
        "無更新不產生報告"
    );

    aggregator.observe(
        "10.0.9.10".parse().expect("位址"),
        "aa:bb:cc:dd:ee:10".to_string(),
    );
    queue.push(aggregator.flush(fixed_time(4)).expect("再觀測有更新"));

    let result = try_flush(&pusher, &queue).await;
    assert_eq!(result, FlushResult::Stored { entries: 2 });

    let reports = server.requests()[0].body["reports"]
        .as_array()
        .expect("reports 為陣列")
        .clone();
    assert_eq!(
        reports
            .iter()
            .map(|report| report["senders"].clone())
            .collect::<Vec<_>>(),
        vec![
            json!([{"address": "10.0.9.9", "mac": "aa:bb:cc:dd:ee:99"}]),
            json!([{"address": "10.0.9.10", "mac": "aa:bb:cc:dd:ee:10"}]),
        ],
        "每輪 flush 只含有自上輪後有更新者"
    );
}

#[tokio::test]
async fn oversized_reports_split_into_requests_under_5000_entries() {
    let server = StubServer::start().await;
    let pusher = Pusher::new(
        reqwest::Client::new(),
        &server.base_url,
        "secret-code",
        agent_info(),
    )
    .expect("建立推送器");
    let queue = PushQueue::new(QUEUE_CAPACITY);

    // 5001 筆 checked：拆為 5000＋1 兩筆請求。
    let base = u32::from_be_bytes([10, 0, 0, 1]);
    let checked = (0..5_001)
        .map(|offset| std::net::Ipv4Addr::from(base + offset))
        .collect();
    queue.push(SweepReport {
        observed_at: "2026-10-06T12:00:00Z".to_string(),
        checked,
        seen: Vec::new(),
    });

    let first = try_flush(&pusher, &queue).await;
    assert_eq!(first, FlushResult::Stored { entries: 5_000 });
    let second = try_flush(&pusher, &queue).await;
    assert_eq!(second, FlushResult::Stored { entries: 1 });
    assert_eq!(queue.entries(), 0);

    let requests = server.requests();
    assert_eq!(requests.len(), 2, "超出 5000 筆拆為兩筆請求");
    assert_eq!(report_entries(&requests[0].body), 5_000);
    assert_eq!(report_entries(&requests[1].body), 1);
    assert!(
        requests
            .iter()
            .all(|request| report_entries(&request.body) <= 5_000),
        "每請求合計 ≤ 5000 筆"
    );
}

#[tokio::test]
async fn failed_push_retries_with_backoff_until_stored() {
    let server = StubServer::start().await;
    server.enqueue(500, json!({"error": "internal"}));
    server.enqueue(503, json!({"error": "unavailable"}));

    let pusher = Pusher::new(
        reqwest::Client::new(),
        &server.base_url,
        "secret-code",
        agent_info(),
    )
    .expect("建立推送器");
    let queue = PushQueue::new(QUEUE_CAPACITY);
    queue.push(sample_report());

    let mut backoff = Backoff::new(Duration::from_millis(10), Duration::from_millis(50));
    let mut delays = Vec::new();
    loop {
        let result = try_flush(&pusher, &queue).await;
        if let FlushResult::Stored { entries } = result {
            assert_eq!(entries, 3);
            break;
        }
        assert!(result.needs_backoff(), "失敗須退避重試：{result:?}");
        let delay = backoff.next_delay();
        delays.push(delay);
        tokio::time::sleep(delay).await;
    }

    assert_eq!(
        delays,
        vec![Duration::from_millis(10), Duration::from_millis(20)],
        "退避自 base 指數成長"
    );
    assert_eq!(server.request_count(), 3, "兩次失敗後第三次成功");
    assert_eq!(queue.entries(), 0, "成功後清空");
    assert_eq!(
        server.requests()[0].body,
        server.requests()[1].body,
        "重試重送同一批內容"
    );
}

#[tokio::test]
async fn unauthorized_keeps_batch_and_reports_explicit_outcome() {
    let server = StubServer::start().await;
    server.enqueue(401, json!({"error": "unauthorized"}));
    server.enqueue(401, json!({"error": "unauthorized"}));

    let pusher = Pusher::new(
        reqwest::Client::new(),
        &server.base_url,
        "wrong-code",
        agent_info(),
    )
    .expect("建立推送器");
    let queue = PushQueue::new(QUEUE_CAPACITY);
    queue.push(sample_report());

    let first = try_flush(&pusher, &queue).await;
    assert_eq!(first, FlushResult::Unauthorized { entries: 3 });
    assert_eq!(queue.entries(), 3, "401 不丟棄批次，等待修正後重送");

    let second = try_flush(&pusher, &queue).await;
    assert_eq!(second, FlushResult::Unauthorized { entries: 3 });
    assert_eq!(server.request_count(), 2, "每次嘗試各送一次請求");
}

#[tokio::test]
async fn subnet_unmatched_drops_batch_and_stops_resending() {
    let server = StubServer::start().await;
    server.enqueue(200, json!({"stored": false, "reason": "subnet_unmatched"}));

    let pusher = Pusher::new(
        reqwest::Client::new(),
        &server.base_url,
        "secret-code",
        agent_info(),
    )
    .expect("建立推送器");
    let queue = PushQueue::new(QUEUE_CAPACITY);
    queue.push(sample_report());

    let result = try_flush(&pusher, &queue).await;
    assert_eq!(result, FlushResult::Dropped { entries: 3 });
    assert_eq!(queue.entries(), 0, "未對應批次丟棄，避免無限重送");

    let again = try_flush(&pusher, &queue).await;
    assert_eq!(again, FlushResult::Empty);
    assert_eq!(server.request_count(), 1, "丟棄後不重送");
}

#[tokio::test]
async fn connection_error_is_retryable_and_keeps_batch() {
    // 取得一個已關閉的埠：綁定後立即放掉。
    let probe_listener = TcpListener::bind("127.0.0.1:0").await.expect("暫用埠");
    let dead_url = format!("http://{}", probe_listener.local_addr().expect("位址"));
    drop(probe_listener);

    let pusher = Pusher::new(
        reqwest::Client::new(),
        &dead_url,
        "secret-code",
        agent_info(),
    )
    .expect("建立推送器");
    let queue = PushQueue::new(QUEUE_CAPACITY);
    queue.push(sample_report());

    let result = try_flush(&pusher, &queue).await;
    assert!(
        matches!(result, FlushResult::Retry { entries: 3, .. }),
        "連線錯誤為可重試失敗：{result:?}"
    );
    assert_eq!(queue.entries(), 3, "失敗保留批次");
}
