//! 環境設定：讀取根目錄 `.env`（`dotenvy`）與行程環境變數。

use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::Context;

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

        Ok(Self {
            bind_addr,
            database_url,
            web_dist_dir,
            kea_api_url,
            kea_api_username,
            kea_api_password,
        })
    }
}
