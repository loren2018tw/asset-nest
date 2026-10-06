//! `/api/v1` 指派候選路由：`GET /ip-candidates`（見 spec §8、票 04）。

use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;

use crate::AppState;
use crate::api::ApiError;
use crate::ip_candidates::{self, Candidates};

/// `limit`：預設 20、夾在 1..=50（見 spec §8）。
const DEFAULT_LIMIT: i64 = 20;
const MAX_LIMIT: i64 = 50;

pub fn router() -> Router<AppState> {
    Router::new().route("/ip-candidates", get(list_candidates))
}

/// 查詢參數；`q` 缺漏由領域層回 400 `invalid_query`（field `q`）。
#[derive(Debug, Deserialize)]
struct CandidatesQuery {
    q: Option<String>,
    limit: Option<i64>,
}

/// 前綴搜尋可用位址（跨網段）與查詢位址狀態；回應不含 `total`（見 spec §8）。
async fn list_candidates(
    State(state): State<AppState>,
    query: Result<Query<CandidatesQuery>, QueryRejection>,
) -> Result<Json<Candidates>, ApiError> {
    let Query(query) = query.map_err(|_| ApiError::validation("查詢參數格式錯誤"))?;
    let limit = query.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let q = query.q.unwrap_or_default();

    let candidates = ip_candidates::find(&state.db, &q, limit as usize).await?;

    Ok(Json(candidates))
}
