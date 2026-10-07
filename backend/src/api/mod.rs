//! API 路由骨架：前綴 `/api`；未來的功能路由一律掛在 `/api/v1` 之下。

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;

use crate::AppState;

mod agents;
mod asset_assignments;
mod assets;
mod auth;
mod error;
mod import;
mod interfaces;
mod ip_candidates;
mod ips;
mod kea;
mod lendings;
mod observations;
mod peer;
mod subnet_import;
mod subnets;

pub use error::ApiError;

/// RFC 5987 `filename*` 值：attr-char 原樣保留，其餘 UTF-8 位元組以 `%XX` 表示。
///
/// 供 CSV 匯出端點的 `Content-Disposition` 共用（見票 01、票 04）。
pub(crate) fn encode_filename(name: &str) -> String {
    name.bytes()
        .map(|byte| match byte {
            b'!' | b'#' | b'$' | b'&' | b'+' | b'-' | b'.' | b'^' | b'_' | b'`' | b'|' | b'~' => {
                (byte as char).to_string()
            }
            byte if byte.is_ascii_alphanumeric() => (byte as char).to_string(),
            byte => format!("%{byte:02X}"),
        })
        .collect()
}

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
            .merge(super::agents::router())
            .merge(super::asset_assignments::router())
            .merge(super::assets::router())
            .merge(super::auth::router())
            .merge(super::import::router())
            .merge(super::interfaces::router())
            .merge(super::ip_candidates::router())
            .merge(super::kea::router())
            .merge(super::lendings::router())
            .merge(super::subnets::router())
            .merge(super::subnet_import::router())
            .merge(super::ips::router())
            .merge(super::observations::router())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_filename_keeps_attr_chars_and_encodes_non_ascii() {
        assert_eq!(
            encode_filename("subnets_export_20261004.csv"),
            "subnets_export_20261004.csv"
        );
        assert_eq!(
            encode_filename("網段匯出_20261004.csv"),
            "%E7%B6%B2%E6%AE%B5%E5%8C%AF%E5%87%BA_20261004.csv"
        );
    }
}
