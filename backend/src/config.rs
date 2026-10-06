//! 環境設定：讀取根目錄 `.env`（`dotenvy`）與行程環境變數。

use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::Context;

/// `OBSERVATION_RETENTION_DAYS` 的預設值（一年；見 ADR-0016）。
const DEFAULT_RETENTION_DAYS: u32 = 365;
/// `AGENT_STALE_SECS` 的預設值（15 分鐘；見票 01、spec §環境設定）。
const DEFAULT_AGENT_STALE_SECS: u64 = 900;
/// `AGENT_AUTH_FAILURE_RETENTION_DAYS` 的預設值（30 天；見票 08、spec §環境設定）。
const DEFAULT_AGENT_AUTH_FAILURE_RETENTION_DAYS: u32 = 30;

/// 後端啟動設定（見 `.env.example`）。
#[derive(Debug, Clone)]
pub struct Config {
    /// HTTP 綁定位址；`BIND_ADDR`，預設 `0.0.0.0:8080`。
    pub bind_addr: SocketAddr,
    /// SQLite 連線字串；`DATABASE_URL`，預設 `sqlite://asset-nest.db`。
    pub database_url: String,
    /// Quasar 建置產物目錄；`WEB_DIST_DIR`，預設 `frontend/dist/spa`。
    pub web_dist_dir: PathBuf,
    /// Kea HTTP 控制通道位址；`KEA_API_URL`，未設定為 `None`（見 `docs/adr/0010`）。
    pub kea_api_url: Option<reqwest::Url>,
    /// Kea 控制通道 Basic 認證帳號；`KEA_API_USERNAME`，未設定為 `None`。
    pub kea_api_username: Option<String>,
    /// Kea 控制通道 Basic 認證密碼；`KEA_API_PASSWORD`（搭配帳號使用）。
    pub kea_api_password: Option<String>,
    /// 代理入庫認證碼；`AGENT_AUTH_CODE`，未設定（或空白）為 `None`＝入庫端點
    /// 一律 503（見 ADR-0019）。
    pub agent_auth_code: Option<String>,
    /// 代理「在線」門檻秒數；`AGENT_STALE_SECS`（正整數，預設 900；見票 01）。
    pub agent_stale_secs: u64,
    /// 代理被拒回報保留天數；`AGENT_AUTH_FAILURE_RETENTION_DAYS`（正整數，
    /// 預設 30；見票 08）。清理 `agent_auth_failure` 的過期列。
    pub agent_auth_failure_retention_days: u32,
    /// 觀測事件保留天數；`OBSERVATION_RETENTION_DAYS`（正整數，預設 365；
    /// 見 `docs/adr/0016`）。僅影響事件歷史，不動 `ip_presence` 現況。
    pub observation_retention_days: u32,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let bind_addr = std::env::var("BIND_ADDR")
            .unwrap_or_else(|_| "0.0.0.0:8080".to_string())
            .parse()
            .context("BIND_ADDR 格式錯誤")?;

        let database_url =
            std::env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite://asset-nest.db".to_string());

        let web_dist_dir = std::env::var("WEB_DIST_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("frontend/dist/spa"));

        let kea_api_url = std::env::var("KEA_API_URL")
            .ok()
            .map(|raw| raw.parse().context("KEA_API_URL 格式錯誤"))
            .transpose()?;

        let kea_api_username = std::env::var("KEA_API_USERNAME").ok();
        let kea_api_password = std::env::var("KEA_API_PASSWORD").ok();
        if kea_api_username.is_none() && kea_api_password.is_some() {
            anyhow::bail!("KEA_API_PASSWORD 已設定但缺少 KEA_API_USERNAME");
        }

        let agent_auth_code = parse_agent_auth_code(std::env::var("AGENT_AUTH_CODE").ok());

        let agent_stale_secs = match std::env::var("AGENT_STALE_SECS") {
            Ok(raw) => parse_agent_stale_secs(&raw)?,
            Err(_) => DEFAULT_AGENT_STALE_SECS,
        };

        let agent_auth_failure_retention_days =
            match std::env::var("AGENT_AUTH_FAILURE_RETENTION_DAYS") {
                Ok(raw) => parse_agent_auth_failure_retention_days(&raw)?,
                Err(_) => DEFAULT_AGENT_AUTH_FAILURE_RETENTION_DAYS,
            };

        let observation_retention_days = match std::env::var("OBSERVATION_RETENTION_DAYS") {
            Ok(raw) => parse_retention_days(&raw)?,
            Err(_) => DEFAULT_RETENTION_DAYS,
        };

        Ok(Self {
            bind_addr,
            database_url,
            web_dist_dir,
            kea_api_url,
            kea_api_username,
            kea_api_password,
            agent_auth_code,
            agent_stale_secs,
            agent_auth_failure_retention_days,
            observation_retention_days,
        })
    }
}

/// 解析 `OBSERVATION_RETENTION_DAYS`：須為正整數，缺值由呼叫端套用預設。
fn parse_retention_days(raw: &str) -> anyhow::Result<u32> {
    let days: u32 = raw.trim().parse().map_err(|_| {
        anyhow::anyhow!("OBSERVATION_RETENTION_DAYS 格式錯誤：{raw}（須為正整數天數，例：365）")
    })?;
    if days == 0 {
        anyhow::bail!("OBSERVATION_RETENTION_DAYS 須為正整數天數（收到 0）");
    }
    Ok(days)
}

/// 解析 `AGENT_AUTH_FAILURE_RETENTION_DAYS`：須為正整數天數，缺值由呼叫端套用預設。
fn parse_agent_auth_failure_retention_days(raw: &str) -> anyhow::Result<u32> {
    let days: u32 = raw.trim().parse().map_err(|_| {
        anyhow::anyhow!(
            "AGENT_AUTH_FAILURE_RETENTION_DAYS 格式錯誤：{raw}（須為正整數天數，例：30）"
        )
    })?;
    if days == 0 {
        anyhow::bail!("AGENT_AUTH_FAILURE_RETENTION_DAYS 須為正整數天數（收到 0）");
    }
    Ok(days)
}

/// 解析 `AGENT_AUTH_CODE`：未設定或空白視為未設定（入庫端點回 503；見 ADR-0019）。
fn parse_agent_auth_code(raw: Option<String>) -> Option<String> {
    raw.map(|code| code.trim().to_string())
        .filter(|code| !code.is_empty())
}

/// 解析 `AGENT_STALE_SECS`：須為正整數秒，缺值由呼叫端套用預設。
fn parse_agent_stale_secs(raw: &str) -> anyhow::Result<u64> {
    let seconds: u64 = raw.trim().parse().map_err(|_| {
        anyhow::anyhow!("AGENT_STALE_SECS 格式錯誤：{raw}（須為正整數秒數，例：900）")
    })?;
    if seconds == 0 {
        anyhow::bail!("AGENT_STALE_SECS 須為正整數秒數（收到 0）");
    }
    Ok(seconds)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retention_days_accepts_positive_integers_and_rejects_invalid() {
        assert_eq!(parse_retention_days("365").expect("合法天數"), 365);
        assert_eq!(parse_retention_days(" 30 ").expect("容許前後空白"), 30);
        assert_eq!(
            parse_retention_days("1").expect("最小合法天數"),
            1,
            "1 天為合法正整數"
        );

        for invalid in ["0", "-1", "abc", "1.5", ""] {
            let error = parse_retention_days(invalid)
                .expect_err(&format!("應拒絕 {invalid:?}"))
                .to_string();
            assert!(
                error.contains("OBSERVATION_RETENTION_DAYS"),
                "錯誤訊息須指明變數（{invalid:?}）：{error}"
            );
        }
    }

    #[test]
    fn retention_days_default_is_one_year() {
        assert_eq!(DEFAULT_RETENTION_DAYS, 365);
    }

    #[test]
    fn auth_failure_retention_accepts_positive_integers_and_rejects_invalid() {
        assert_eq!(
            parse_agent_auth_failure_retention_days("30").expect("合法天數"),
            30
        );
        assert_eq!(
            parse_agent_auth_failure_retention_days(" 7 ").expect("容許空白"),
            7
        );
        assert_eq!(
            parse_agent_auth_failure_retention_days("1").expect("最小合法天數"),
            1
        );

        for invalid in ["0", "-1", "abc", "1.5", ""] {
            let error = parse_agent_auth_failure_retention_days(invalid)
                .expect_err(&format!("應拒絕 {invalid:?}"))
                .to_string();
            assert!(
                error.contains("AGENT_AUTH_FAILURE_RETENTION_DAYS"),
                "錯誤訊息須指明變數（{invalid:?}）：{error}"
            );
        }
    }

    #[test]
    fn auth_failure_retention_default_is_30_days() {
        assert_eq!(DEFAULT_AGENT_AUTH_FAILURE_RETENTION_DAYS, 30);
    }

    #[test]
    fn agent_stale_secs_accepts_positive_integers_and_rejects_invalid() {
        assert_eq!(parse_agent_stale_secs("900").expect("合法秒數"), 900);
        assert_eq!(parse_agent_stale_secs(" 60 ").expect("容許空白"), 60);
        assert_eq!(parse_agent_stale_secs("1").expect("最小合法秒數"), 1);

        for invalid in ["0", "-1", "abc", "1.5", ""] {
            let error = parse_agent_stale_secs(invalid)
                .expect_err(&format!("應拒絕 {invalid:?}"))
                .to_string();
            assert!(
                error.contains("AGENT_STALE_SECS"),
                "錯誤訊息須指明變數（{invalid:?}）：{error}"
            );
        }
    }

    #[test]
    fn agent_stale_default_is_900() {
        assert_eq!(DEFAULT_AGENT_STALE_SECS, 900);
    }

    #[test]
    fn agent_auth_code_missing_or_blank_is_unset() {
        assert_eq!(parse_agent_auth_code(None), None, "未設定＝未啟用");
        assert_eq!(
            parse_agent_auth_code(Some(String::new())),
            None,
            "空字串視為未設定"
        );
        assert_eq!(
            parse_agent_auth_code(Some("   ".to_string())),
            None,
            "全空白視為未設定"
        );
        assert_eq!(
            parse_agent_auth_code(Some("  secret ".to_string())),
            Some("secret".to_string()),
            "前後空白應去除"
        );
    }
}
