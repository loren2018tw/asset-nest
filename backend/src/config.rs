//! 環境設定：讀取根目錄 `.env`（`dotenvy`）與行程環境變數。

use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::Context;

use crate::probe::ProbeMode;

/// `OBSERVATION_RETENTION_DAYS` 的預設值（一年；見 ADR-0016）。
const DEFAULT_RETENTION_DAYS: u32 = 365;

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
    /// 觀測探測模式；`OBSERVATION_PROBE_MODE`（`auto|raw|unprivileged`，預設 `auto`；
    /// 見 `docs/adr/0015`）。
    pub observation_probe_mode: ProbeMode,
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

        let observation_probe_mode = match std::env::var("OBSERVATION_PROBE_MODE") {
            Ok(raw) => ProbeMode::parse(&raw).ok_or_else(|| {
                anyhow::anyhow!(
                    "OBSERVATION_PROBE_MODE 格式錯誤：{raw}（僅接受 auto／raw／unprivileged）"
                )
            })?,
            Err(_) => ProbeMode::default(),
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
            observation_probe_mode,
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
}
