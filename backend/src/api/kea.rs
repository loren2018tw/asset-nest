//! `/api/v1` Kea 同步路由（見 `docs/adr/0011`）。
//!
//! `GET /kea/sync/plan`：完整同步計畫（dry-run、唯讀）。
//! `POST /kea/sync`：重算計畫並套用；回傳每網段增／改／刪計數、失敗清單與
//! `config-write` 狀態。`KEA_API_URL` 未設定時回 400。

use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};

use crate::AppState;
use crate::api::ApiError;
use crate::kea::http::Client;
use crate::kea::sync::{self, SyncApplyReport, SyncPlan};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/kea/sync/plan", get(sync_plan))
        .route("/kea/sync", post(apply_sync))
}

/// 取得 Kea client；未設定時回 400（前端據訊息提示）。
fn client(state: &AppState) -> Result<&Client, ApiError> {
    state
        .kea
        .as_ref()
        .ok_or_else(|| ApiError::validation("Kea 未設定（KEA_API_URL），無法同步"))
}

async fn sync_plan(State(state): State<AppState>) -> Result<Json<SyncPlan>, ApiError> {
    let plan = sync::plan(&state.db, client(&state)?).await?;
    Ok(Json(plan))
}

async fn apply_sync(State(state): State<AppState>) -> Result<Json<SyncApplyReport>, ApiError> {
    let report = sync::apply(&state.db, client(&state)?).await?;
    Ok(Json(report))
}
