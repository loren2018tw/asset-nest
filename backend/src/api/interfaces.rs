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
use crate::interfaces::{self, Interface, InterfaceInput, InterfacePatch, Warning};

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

    let interface = interfaces::update(&state.db, id, valid)
        .await
        .map_err(|error| ApiError::internal("更新介面失敗", error))?
        .ok_or_else(|| ApiError::not_found("找不到介面"))?;

    let warnings = interfaces::mac_warnings(&state.db, interface.mac.as_deref(), id)
        .await
        .map_err(|error| ApiError::internal("檢查 MAC 重複失敗", error))?;

    Ok(Json(InterfaceResponse {
        interface,
        warnings,
    }))
}

async fn delete_interface(
    State(state): State<AppState>,
    id: Result<Path<i64>, PathRejection>,
) -> Result<StatusCode, ApiError> {
    let Path(id) = id.map_err(|_| ApiError::validation("介面 id 格式錯誤"))?;

    let deleted = interfaces::delete(&state.db, id)
        .await
        .map_err(|error| ApiError::internal("刪除介面失敗", error))?;

    if deleted {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::not_found("找不到介面"))
    }
}
