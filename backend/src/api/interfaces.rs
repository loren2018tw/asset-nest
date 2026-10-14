//! `/api/v1` 介面管理路由（見 spec §2.2、§5）。
//!
//! 介面一律由資產對話框管理：新增掛在資產底下，編輯／刪除以介面 id 操作。

use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{patch, post};
use axum::{Json, Router};
use serde::Serialize;

use crate::AppState;
use crate::api::ApiError;
use crate::assets;
use crate::assignments;
use crate::interfaces::{self, Interface, InterfaceInput, InterfacePatch, Warning};
use crate::kea::sync as kea_sync;
use crate::kea::sync::KeaSync;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/assets/{id}/interfaces", post(create_interface))
        .route(
            "/interfaces/{id}",
            patch(update_interface).delete(delete_interface),
        )
}

/// 介面儲存回應：介面欄位攤平，加上不阻擋的語意警示（見 ADR-0006）。
#[derive(Debug, Serialize)]
struct InterfaceResponse {
    #[serde(flatten)]
    interface: Interface,
    warnings: Vec<Warning>,
    /// MAC 變更時的 Kea 推送結果；未涉同步時不顯示（見 ADR-0027）。
    #[serde(skip_serializing_if = "Option::is_none")]
    kea_sync: Option<Vec<KeaSync>>,
}

/// 刪除介面回應：受管網段保留的 Kea 刪除推送結果（見 ADR-0027）。
#[derive(Debug, Serialize)]
struct InterfaceDeleted {
    kea_sync: Vec<KeaSync>,
}

async fn create_interface(
    State(state): State<AppState>,
    id: Result<Path<i64>, PathRejection>,
    payload: Result<Json<InterfaceInput>, JsonRejection>,
) -> Result<(StatusCode, Json<InterfaceResponse>), ApiError> {
    let Path(asset_id) = id.map_err(|_| ApiError::validation("資產 id 格式錯誤"))?;
    let Json(input) = payload.map_err(|_| ApiError::validation("請求內容格式錯誤"))?;
    let valid = input.validate()?;

    if assets::get(&state.db, asset_id)
        .await
        .map_err(|error| ApiError::internal("讀取資產失敗", error))?
        .is_none()
    {
        return Err(ApiError::not_found("找不到資產"));
    }

    let interface = interfaces::create(&state.db, asset_id, valid)
        .await
        .map_err(|error| ApiError::internal("新增介面失敗", error))?;

    let warnings = interfaces::mac_warnings(&state.db, interface.mac.as_deref(), interface.id)
        .await
        .map_err(|error| ApiError::internal("檢查 MAC 重複失敗", error))?;

    Ok((
        StatusCode::CREATED,
        Json(InterfaceResponse {
            interface,
            warnings,
            kea_sync: None,
        }),
    ))
}

async fn update_interface(
    State(state): State<AppState>,
    id: Result<Path<i64>, PathRejection>,
    payload: Result<Json<InterfacePatch>, JsonRejection>,
) -> Result<Json<InterfaceResponse>, ApiError> {
    let Path(id) = id.map_err(|_| ApiError::validation("介面 id 格式錯誤"))?;
    let Json(patch) = payload.map_err(|_| ApiError::validation("請求內容格式錯誤"))?;

    let existing = interfaces::get(&state.db, id)
        .await
        .map_err(|error| ApiError::internal("讀取介面失敗", error))?
        .ok_or_else(|| ApiError::not_found("找不到介面"))?;

    let valid = patch.apply_to(&existing)?;

    // 結構規則：有保留指派的介面不可清空 MAC（見 ADR-0027；與「保留需介面
    // 有 MAC」一致，ADR-0006）。
    if existing.mac.is_some() && valid.mac.is_none() {
        let count = assignments::list_reservations_for_interfaces(&state.db, &[id])
            .await
            .map_err(|error| ApiError::internal("讀取保留指派失敗", error))?
            .len();
        if count > 0 {
            return Err(ApiError::validation(format!(
                "此介面有 {count} 筆保留指派，清空 MAC 前請先取消或改用途"
            ))
            .field("mac"));
        }
    }

    let interface = interfaces::update(&state.db, id, valid)
        .await
        .map_err(|error| ApiError::internal("更新介面失敗", error))?
        .ok_or_else(|| ApiError::not_found("找不到介面"))?;

    // MAC 變更：以新 MAC 重新推送該介面在受管網段的保留（見 ADR-0027）；
    // 僅在確有推送時附上結果。
    let kea_sync = if existing.mac != interface.mac {
        let reservations = assignments::list_reservations_for_interfaces(&state.db, &[id])
            .await
            .map_err(|error| ApiError::internal("讀取保留指派失敗", error))?;
        let syncs =
            kea_sync::push_reservation_updates(&state.db, state.kea.as_ref(), &reservations)
                .await?;
        if syncs.is_empty() { None } else { Some(syncs) }
    } else {
        None
    };

    let warnings = interfaces::mac_warnings(&state.db, interface.mac.as_deref(), id)
        .await
        .map_err(|error| ApiError::internal("檢查 MAC 重複失敗", error))?;

    Ok(Json(InterfaceResponse {
        interface,
        warnings,
        kea_sync,
    }))
}

async fn delete_interface(
    State(state): State<AppState>,
    id: Result<Path<i64>, PathRejection>,
) -> Result<Json<InterfaceDeleted>, ApiError> {
    let Path(id) = id.map_err(|_| ApiError::validation("介面 id 格式錯誤"))?;

    // 先擷取保留（刪除後指派連動消失），本地刪除後逐筆推送 Kea（見 ADR-0027）。
    let reservations = assignments::list_reservations_for_interfaces(&state.db, &[id])
        .await
        .map_err(|error| ApiError::internal("讀取保留指派失敗", error))?;

    let deleted = interfaces::delete(&state.db, id)
        .await
        .map_err(|error| ApiError::internal("刪除介面失敗", error))?;

    if !deleted {
        return Err(ApiError::not_found("找不到介面"));
    }

    let kea_sync =
        kea_sync::push_reservation_deletes(&state.db, state.kea.as_ref(), &reservations).await?;

    Ok(Json(InterfaceDeleted { kea_sync }))
}
