//! asset-nest 後端：API 與前端靜態檔的同源服務（見 `docs/adr/0004`）。

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
pub mod ips;
pub mod kea;
pub mod observation;
pub mod peer;
pub mod probe;
pub mod subnet_import;
pub mod subnets;
pub mod web;

use std::path::PathBuf;
use std::sync::Arc;

use axum::Router;
use axum::middleware;
use sqlx::SqlitePool;
use tower_http::trace::TraceLayer;

use crate::probe::{Prober, SystemProber};

/// 共用狀態：注入所有 handler。
#[derive(Clone)]
pub struct AppState {
    pub db: SqlitePool,
    pub web_dist_dir: PathBuf,
    /// Kea 控制通道 client；`KEA_API_URL` 未設定時為 `None`（見 `docs/adr/0010`）。
    pub kea: Option<kea::http::Client>,
    /// 觀測探測邊界；測試以 [`AppState::with_prober`] 注入 stub（見 ADR-0015）。
    pub prober: Arc<dyn Prober + Send + Sync>,
    /// 探索掃描每秒最多送出的探測數（`OBSERVATION_DISCOVERY_RATE_PPS`；
    /// 手動探索路徑用，預設 1000；見票 05）。
    pub discovery_rate_pps: u32,
    /// 探索掃描時被動 ARP 監聽窗長秒數（`OBSERVATION_PASSIVE_WINDOW_SECS`；
    /// 0＝停用；手動與排程探索用，預設 60；見票 01、ADR-0017）。
    pub passive_window_secs: u64,
}

impl AppState {
    pub fn new(db: SqlitePool, web_dist_dir: impl Into<PathBuf>) -> Self {
        Self {
            db,
            web_dist_dir: web_dist_dir.into(),
            kea: None,
            prober: Arc::new(SystemProber::new()),
            discovery_rate_pps: 1_000,
            passive_window_secs: 60,
        }
    }

    /// 附掛 Kea client（正式啟動與 Kea 同步整合測試用）。
    pub fn with_kea(mut self, client: kea::http::Client) -> Self {
        self.kea = Some(client);
        self
    }

    /// 附掛探測邊界（整合測試注入 stub 用；正式啟動維持預設 [`SystemProber`]）。
    pub fn with_prober(mut self, prober: Arc<dyn Prober + Send + Sync>) -> Self {
        self.prober = prober;
        self
    }

    /// 設定探索掃描速率上限（正式啟動帶入設定；測試可覆寫，見票 05）。
    pub fn with_discovery_rate_pps(mut self, rate_pps: u32) -> Self {
        self.discovery_rate_pps = rate_pps;
        self
    }

    /// 設定探索時被動監聽窗長秒數（正式啟動帶入設定；測試可覆寫，見票 01）。
    pub fn with_passive_window_secs(mut self, window_secs: u64) -> Self {
        self.passive_window_secs = window_secs;
        self
    }
}

/// 組裝應用程式路由。
pub fn app(state: AppState) -> Router {
    let api = Router::new()
        .nest("/api", api::router())
        .with_state(state.clone())
        .layer(middleware::from_fn(auth::noop_auth));

    api.fallback_service(web::service(&state.web_dist_dir))
        .layer(TraceLayer::new_for_http())
}
