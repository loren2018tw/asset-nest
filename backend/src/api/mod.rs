//! API 路由骨架：前綴 `/api`；未來的功能路由一律掛在 `/api/v1` 之下。

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;

use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/health", get(health))
        .nest("/v1", v1::router())
}

/// 功能階段的路由掛載點。
mod v1 {
    use axum::Router;

    use crate::AppState;

    pub fn router() -> Router<AppState> {
        Router::new()
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
