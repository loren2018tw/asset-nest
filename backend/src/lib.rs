//! asset-nest 後端：API 與前端靜態檔的同源服務（見 `docs/adr/0004`）。

pub mod agents;
pub mod api;
pub mod asset_export;
pub mod assets;
pub mod assignments;
pub mod auth;
pub mod config;
pub mod conflicts;
pub mod db;
pub mod import;
pub mod interfaces;
pub mod ip_candidates;
pub mod ips;
pub mod kea;
pub mod lendings;
pub mod observation;
pub mod peer;
pub mod subnet_import;
pub mod subnets;
pub mod web;

use std::path::PathBuf;

use axum::{Router, middleware};
use sqlx::SqlitePool;
use tower_http::trace::TraceLayer;

/// 共用狀態：注入所有 handler。
#[derive(Clone)]
pub struct AppState {
    pub db: SqlitePool,
    pub web_dist_dir: PathBuf,
    /// Kea 控制通道 client；`KEA_API_URL` 未設定時為 `None`（見 `docs/adr/0010`）。
    pub kea: Option<kea::http::Client>,
    /// 代理入庫認證碼（`AGENT_AUTH_CODE`）；`None`＝未設定，入庫端點回 503
    /// （見 `docs/adr/0019`）。
    pub agent_auth_code: Option<String>,
    /// 代理「在線」門檻秒數（`AGENT_STALE_SECS`；見票 01）。
    pub agent_stale_secs: u64,
    /// 登入認證設定；`None`＝不附掛登入（測試路徑），middleware 全數放行
    /// （見 `docs/adr/0021`、spec §3.3）。
    pub auth: Option<auth::AuthConfig>,
}

impl AppState {
    pub fn new(db: SqlitePool, web_dist_dir: impl Into<PathBuf>) -> Self {
        Self {
            db,
            web_dist_dir: web_dist_dir.into(),
            kea: None,
            agent_auth_code: None,
            agent_stale_secs: 900,
            auth: None,
        }
    }

    /// 附掛登入認證；正式啟動一律附掛（未設定即 `admin/admin`），
    /// `new()` 維持不附掛供既有測試使用（見 spec §3.3）。
    pub fn with_auth(mut self, username: impl Into<String>, password: impl Into<String>) -> Self {
        self.auth = Some(auth::AuthConfig {
            username: username.into(),
            password: password.into(),
        });
        self
    }

    /// 附掛代理入庫認證碼；`None` 維持未設定（入庫端點 503；見 ADR-0019）。
    pub fn with_agent_auth_code(mut self, auth_code: Option<String>) -> Self {
        self.agent_auth_code = auth_code;
        self
    }

    /// 設定代理「在線」門檻秒數（正式啟動帶入設定；測試可覆寫，見票 01）。
    pub fn with_agent_stale_secs(mut self, stale_secs: u64) -> Self {
        self.agent_stale_secs = stale_secs;
        self
    }

    /// 附掛 Kea client（正式啟動與 Kea 同步整合測試用）。
    pub fn with_kea(mut self, client: kea::http::Client) -> Self {
        self.kea = Some(client);
        self
    }
}

/// 組裝應用程式路由。
pub fn app(state: AppState) -> Router {
    let api = Router::new()
        .nest("/api", api::router())
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::require_login,
        ))
        .with_state(state.clone());

    api.fallback_service(web::service(&state.web_dist_dir))
        .layer(TraceLayer::new_for_http())
}
