//! 代理心跳：`POST {AGENT_SERVER_URL}/api/v1/agents/heartbeat`
//! （標頭 `X-Auth-Code`；見 spec §心跳）。
//!
//! 啟動後立即送一次、之後每 [`HEARTBEAT_INTERVAL`] 送一次；回應
//! `subnet_matched:false` 只在狀態變化時記一次警告（不重複洗版）。

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::push::{AgentInfo, endpoint};

/// 心跳間隔（60 秒；見 spec §心跳）。
pub const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(60);

/// 心跳端點路徑。
const HEARTBEAT_PATH: &str = "/api/v1/agents/heartbeat";
/// 代理認證碼標頭（與後端 `api::agents` 一致）。
const AUTH_CODE_HEADER: &str = "X-Auth-Code";

/// 心跳用戶端。
pub struct Heartbeat {
    client: reqwest::Client,
    endpoint: String,
    auth_code: String,
    agent: AgentInfo,
}

impl Heartbeat {
    /// 建立心跳用戶端；`server_url` 須為合法 http(s) 網址。
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
            endpoint: endpoint(server_url, HEARTBEAT_PATH),
            auth_code: auth_code.to_string(),
            agent,
        })
    }

    /// 送出一次心跳；回傳後端回應的 `subnet_matched`。
    pub async fn send(&self) -> anyhow::Result<bool> {
        let body = HeartbeatBody {
            instance_id: &self.agent.instance_id,
            name: &self.agent.name,
            version: &self.agent.version,
            subnet_cidr: &self.agent.subnet_cidr,
        };

        let response = self
            .client
            .post(&self.endpoint)
            .header(AUTH_CODE_HEADER, &self.auth_code)
            .json(&body)
            .send()
            .await
            .map_err(|error| anyhow::anyhow!("心跳請求失敗：{error}"))?;

        let status = response.status();
        if !status.is_success() {
            return Err(anyhow::anyhow!("後端回覆 HTTP {status}"));
        }

        let parsed: HeartbeatResponse = response
            .json()
            .await
            .map_err(|error| anyhow::anyhow!("心跳回應解析失敗：{error}"))?;
        Ok(parsed.subnet_matched)
    }
}

/// 心跳請求 body（見 spec §HTTP API）。
#[derive(Debug, Serialize)]
struct HeartbeatBody<'a> {
    instance_id: &'a str,
    name: &'a str,
    version: &'a str,
    subnet_cidr: &'a str,
}

/// 心跳回應（見 `backend/src/api/agents.rs`）。
#[derive(Debug, Deserialize)]
struct HeartbeatResponse {
    #[serde(default)]
    subnet_matched: bool,
}

/// 心跳對應狀態的變化（供「狀態變化才重印警告」判斷）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchChange {
    /// 首次回報此狀態；`false` 視為變化（第一次即警告）。
    First(bool),
    /// 與上次相同，不重印。
    Unchanged,
    /// 由未對應恢復為已對應。
    NowMatched,
    /// 由已對應變為未對應。
    NowMismatched,
}

/// 記錄最近一次 `subnet_matched`，據以判斷是否需警告。
#[derive(Debug, Default)]
pub struct MatchTracker {
    last: Option<bool>,
}

impl MatchTracker {
    /// 觀察一次回應並回傳狀態變化。
    pub fn observe(&mut self, matched: bool) -> MatchChange {
        let change = match self.last {
            None => MatchChange::First(matched),
            Some(previous) if previous == matched => MatchChange::Unchanged,
            Some(false) => MatchChange::NowMatched,
            Some(true) => MatchChange::NowMismatched,
        };
        self.last = Some(matched);
        change
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn match_tracker_warns_once_per_state_change() {
        let mut tracker = MatchTracker::default();

        assert_eq!(tracker.observe(true), MatchChange::First(true));
        assert_eq!(
            tracker.observe(true),
            MatchChange::Unchanged,
            "持續對應不重印"
        );
        assert_eq!(
            tracker.observe(false),
            MatchChange::NowMismatched,
            "轉為未對應時警告一次"
        );
        assert_eq!(
            tracker.observe(false),
            MatchChange::Unchanged,
            "持續未對應不重複警告"
        );
        assert_eq!(tracker.observe(true), MatchChange::NowMatched, "恢復對應");
        assert_eq!(tracker.observe(true), MatchChange::Unchanged);
        assert_eq!(
            tracker.observe(false),
            MatchChange::NowMismatched,
            "再次未對應仍算狀態變化"
        );
    }

    #[test]
    fn match_tracker_first_false_counts_as_change() {
        let mut tracker = MatchTracker::default();
        assert_eq!(
            tracker.observe(false),
            MatchChange::First(false),
            "首次即未對應須警告"
        );
        assert_eq!(tracker.observe(false), MatchChange::Unchanged);
    }
}
