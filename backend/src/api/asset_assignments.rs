//! `/api/v1` 資產端指派路由（見票 10、ADR-0007）。
//!
//! `PUT /assets/{id}/assignments`：由位址反推所屬網段後指派給該資產的介面。
//! 位址已指派給其他介面時，先以 400 `address_assigned_elsewhere` 回目前對象
//! （供前端確認），確認後帶 `transfer: true` 重試，於單一交易內原子移轉。
//! 回應為既有 `Assignment` 欄位＋`warnings`＋`transferred`＋`kea_sync`
//! （受管網段＋保留用途時推送 Kea；見 `docs/adr/0011`）。

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
use crate::kea::sync as kea_sync;
use crate::kea::sync::KeaSync;
use crate::subnets;

pub fn router() -> Router<AppState> {
    Router::new().route("/assets/{id}/assignments", put(put_asset_assignment))
}

/// 資產端指派回應：指派欄位攤平＋不阻擋的語意警示＋是否發生移轉＋Kea 推送結果。
#[derive(Debug, Serialize)]
struct AssetAssignmentResponse {
    #[serde(flatten)]
    assignment: Assignment,
    warnings: Vec<Warning>,
    /// 是否發生移轉（原指派已取消、位址改派給目前介面）。
    transferred: bool,
    /// Kea 單筆推送結果；僅在應同步時出現（見 `docs/adr/0011`）。
    #[serde(skip_serializing_if = "Option::is_none")]
    kea_sync: Option<KeaSync>,
}

async fn put_asset_assignment(
    State(state): State<AppState>,
    id: Result<Path<i64>, PathRejection>,
    payload: Result<Json<AssetAssignmentInput>, JsonRejection>,
) -> Result<Json<AssetAssignmentResponse>, ApiError> {
    let Path(asset_id) = id.map_err(|_| ApiError::validation("資產 id 格式錯誤"))?;
    let Json(input) = payload.map_err(|_| ApiError::validation("請求內容格式錯誤"))?;

    let (address, transfer, valid) = input.validate()?;

    // 同步判斷需要異動前狀態（移轉時為舊介面的保留）。
    let before = match subnets::find_by_address(&state.db, address).await? {
        Some(subnet) => assignments::find(&state.db, subnet.id, &address.to_string())
            .await
            .map_err(|error| ApiError::internal("讀取指派失敗", error))?,
        None => None,
    };

    let (assignment, transferred) =
        assignments::assign_for_asset(&state.db, asset_id, address, valid, transfer).await?;

    // 警示需以網段（含 pool）重新偵測；指派本身已驗證存在。
    let subnet = subnets::get(&state.db, assignment.subnet_id)
        .await
        .map_err(|error| ApiError::internal("讀取網段失敗", error))?
        .ok_or_else(|| ApiError::internal("讀取網段失敗", "網段不存在"))?;

    let warnings = warnings_for_address(&state, &subnet, &assignment.address).await?;
    let kea_sync = kea_sync::after_assignment_change(
        &state.db,
        state.kea.as_ref(),
        &subnet,
        before.as_ref(),
        Some(&assignment),
    )
    .await?;

    Ok(Json(AssetAssignmentResponse {
        assignment,
        warnings,
        transferred,
        kea_sync,
    }))
}
