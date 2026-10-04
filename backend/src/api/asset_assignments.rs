//! `/api/v1` 資產端指派路由（見票 10、ADR-0007）。
//!
//! `PUT /assets/{id}/assignments`：由位址反推所屬網段後指派給該資產的介面。
//! 位址已指派給其他介面時，先以 400 `address_assigned_elsewhere` 回目前對象
//! （供前端確認），確認後帶 `transfer: true` 重試，於單一交易內原子移轉。
//! 回應為既有 `Assignment` 欄位＋`warnings`＋`transferred`。

use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use axum::routing::put;
use axum::{Json, Router};
use serde::Serialize;

use crate::AppState;
use crate::api::ApiError;
use crate::api::ips::warnings_for_address;
use crate::assignments::{self, AssetAssignmentInput, Assignment};
use crate::interfaces::Warning;
use crate::subnets;

pub fn router() -> Router<AppState> {
    Router::new().route("/assets/{id}/assignments", put(put_asset_assignment))
}

/// 資產端指派回應：指派欄位攤平＋不阻擋的語意警示＋是否發生移轉。
#[derive(Debug, Serialize)]
struct AssetAssignmentResponse {
    #[serde(flatten)]
    assignment: Assignment,
    warnings: Vec<Warning>,
    /// 是否發生移轉（原指派已取消、位址改派給目前介面）。
    transferred: bool,
}

async fn put_asset_assignment(
    State(state): State<AppState>,
    id: Result<Path<i64>, PathRejection>,
    payload: Result<Json<AssetAssignmentInput>, JsonRejection>,
) -> Result<Json<AssetAssignmentResponse>, ApiError> {
    let Path(asset_id) = id.map_err(|_| ApiError::validation("資產 id 格式錯誤"))?;
    let Json(input) = payload.map_err(|_| ApiError::validation("請求內容格式錯誤"))?;

    let (address, transfer, valid) = input.validate()?;
    let (assignment, transferred) =
        assignments::assign_for_asset(&state.db, asset_id, address, valid, transfer).await?;

    // 警示需以網段（含 pool）重新偵測；指派本身已驗證存在。
    let subnet = subnets::get(&state.db, assignment.subnet_id)
        .await
        .map_err(|error| ApiError::internal("讀取網段失敗", error))?
        .ok_or_else(|| ApiError::internal("讀取網段失敗", "網段不存在"))?;

    let warnings = warnings_for_address(&state, &subnet, &assignment.address).await?;

    Ok(Json(AssetAssignmentResponse {
        assignment,
        warnings,
        transferred,
    }))
}
