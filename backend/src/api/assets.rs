//! `/api/v1` 資產管理路由（見 spec §4.1、§5）。

use axum::extract::rejection::{JsonRejection, PathRejection, QueryRejection};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use crate::AppState;
use crate::api::ApiError;
use crate::assets::{self, Asset, AssetFilter, AssetInput, AssetPatch};

/// 清單預設每頁筆數（見 spec §6）。
const DEFAULT_PER_PAGE: i64 = 50;
/// 每頁筆數上限，避免單次拉取過量資料。
const MAX_PER_PAGE: i64 = 200;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/assets", get(list_assets).post(create_asset))
        .route(
            "/assets/{id}",
            get(get_asset).patch(update_asset).delete(delete_asset),
        )
        .route("/locations", get(list_locations))
        .route("/brands", get(list_brands))
}

#[derive(Debug, Deserialize)]
struct ListQuery {
    q: Option<String>,
    location: Option<String>,
    brand: Option<String>,
    device_serial: Option<String>,
    page: Option<i64>,
    per_page: Option<i64>,
}

#[derive(Debug, Serialize)]
struct AssetPage {
    items: Vec<Asset>,
    total: i64,
    page: i64,
    per_page: i64,
}

#[derive(Debug, Serialize)]
struct StringItems {
    items: Vec<String>,
}

async fn list_assets(
    State(state): State<AppState>,
    query: Result<Query<ListQuery>, QueryRejection>,
) -> Result<Json<AssetPage>, ApiError> {
    let Query(query) = query.map_err(|_| ApiError::validation("查詢參數格式錯誤"))?;

    let page = query.page.unwrap_or(1).max(1);
    let per_page = query
        .per_page
        .unwrap_or(DEFAULT_PER_PAGE)
        .clamp(1, MAX_PER_PAGE);

    let filter = AssetFilter {
        q: query.q,
        location: query.location,
        brand: query.brand,
        device_serial: query.device_serial,
        page,
        per_page,
    };

    let (items, total) = assets::list(&state.db, &filter)
        .await
        .map_err(|error| ApiError::internal("讀取資產清單失敗", error))?;

    Ok(Json(AssetPage {
        items,
        total,
        page,
        per_page,
    }))
}

async fn get_asset(
    State(state): State<AppState>,
    id: Result<Path<i64>, PathRejection>,
) -> Result<Json<Asset>, ApiError> {
    let Path(id) = id.map_err(|_| ApiError::validation("資產 id 格式錯誤"))?;

    assets::get(&state.db, id)
        .await
        .map_err(|error| ApiError::internal("讀取資產失敗", error))?
        .map(Json)
        .ok_or_else(|| ApiError::not_found("找不到資產"))
}

async fn create_asset(
    State(state): State<AppState>,
    payload: Result<Json<AssetInput>, JsonRejection>,
) -> Result<(StatusCode, Json<Asset>), ApiError> {
    let Json(input) = payload.map_err(|_| ApiError::validation("請求內容格式錯誤"))?;
    let valid = input.validate()?;

    let asset = assets::create(&state.db, valid)
        .await
        .map_err(|error| ApiError::internal("新增資產失敗", error))?;

    Ok((StatusCode::CREATED, Json(asset)))
}

async fn update_asset(
    State(state): State<AppState>,
    id: Result<Path<i64>, PathRejection>,
    payload: Result<Json<AssetPatch>, JsonRejection>,
) -> Result<Json<Asset>, ApiError> {
    let Path(id) = id.map_err(|_| ApiError::validation("資產 id 格式錯誤"))?;
    let Json(patch) = payload.map_err(|_| ApiError::validation("請求內容格式錯誤"))?;
    let valid = patch.validate()?;

    assets::update(&state.db, id, valid)
        .await
        .map_err(|error| ApiError::internal("更新資產失敗", error))?
        .map(Json)
        .ok_or_else(|| ApiError::not_found("找不到資產"))
}

async fn delete_asset(
    State(state): State<AppState>,
    id: Result<Path<i64>, PathRejection>,
) -> Result<StatusCode, ApiError> {
    let Path(id) = id.map_err(|_| ApiError::validation("資產 id 格式錯誤"))?;

    let deleted = assets::delete(&state.db, id)
        .await
        .map_err(|error| ApiError::internal("刪除資產失敗", error))?;

    if deleted {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::not_found("找不到資產"))
    }
}

async fn list_locations(State(state): State<AppState>) -> Result<Json<StringItems>, ApiError> {
    let items = assets::locations(&state.db)
        .await
        .map_err(|error| ApiError::internal("讀取位置清單失敗", error))?;
    Ok(Json(StringItems { items }))
}

async fn list_brands(State(state): State<AppState>) -> Result<Json<StringItems>, ApiError> {
    let items = assets::brands(&state.db)
        .await
        .map_err(|error| ApiError::internal("讀取廠牌清單失敗", error))?;
    Ok(Json(StringItems { items }))
}
