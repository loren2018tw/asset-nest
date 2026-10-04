//! API 路由骨架：前綴 `/api`；未來的功能路由一律掛在 `/api/v1` 之下。

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;

use crate::AppState;

mod asset_assignments;
mod assets;
mod error;
mod import;
mod interfaces;
mod ips;
mod peer;
mod subnets;

pub use error::ApiError;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/health", get(health))
        .nest("/v1", v1::router())
        .fallback(not_found)
}

/// 功能階段的路由掛載點。
mod v1 {
    use axum::Router;

    use crate::AppState;

    pub fn router() -> Router<AppState> {
        Router::new()
            .merge(super::asset_assignments::router())
            .merge(super::assets::router())
            .merge(super::import::router())
            .merge(super::interfaces::router())
            .merge(super::subnets::router())
            .merge(super::ips::router())
            .merge(super::peer::router())
    }
}

#[derive(Serialize)]
struct Health {
    status: &'static str,
    service: &'static str,
    version: &'static str,
    database: &'static str,
}

async fn health(State(state): State<AppState>) -> Json<Health> {
    let database = match sqlx::query_scalar::<_, i64>("SELECT 1")
        .fetch_one(&state.db)
        .await
    {
        Ok(_) => "ok",
        Err(_) => "unavailable",
    };

    Json(Health {
        status: "ok",
        service: "asset-nest",
        version: env!("CARGO_PKG_VERSION"),
        database,
    })
}

/// `/api` 底下未匹配的路徑：一律 JSON 404，不落入 SPA fallback。
async fn not_found() -> ApiError {
    ApiError::not_found("找不到此 API 路徑")
}
