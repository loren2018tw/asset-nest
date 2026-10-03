//! asset-nest 後端：API 與前端靜態檔的同源服務（見 `docs/adr/0004`）。

pub mod api;
pub mod assets;
pub mod assignments;
pub mod auth;
pub mod config;
pub mod db;
pub mod interfaces;
pub mod ips;
pub mod kea;
pub mod subnets;
pub mod web;

use std::path::PathBuf;

use axum::Router;
use axum::middleware;
use sqlx::SqlitePool;
use tower_http::trace::TraceLayer;

/// 共用狀態：注入所有 handler。
#[derive(Clone)]
pub struct AppState {
    pub db: SqlitePool,
    pub web_dist_dir: PathBuf,
}

impl AppState {
    pub fn new(db: SqlitePool, web_dist_dir: impl Into<PathBuf>) -> Self {
        Self {
            db,
            web_dist_dir: web_dist_dir.into(),
        }
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
