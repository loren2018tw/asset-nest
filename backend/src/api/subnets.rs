//! `/api/v1` 網段設定路由（見 spec §4.2、§5）。

use axum::body::Body;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use axum::response::Response;
use axum::routing::get;
use axum::{Json, Router};
use chrono::Local;
use serde::Serialize;

use crate::AppState;
use crate::api::{ApiError, encode_filename};
use crate::subnets::{self, Subnet, SubnetInput, SubnetPatch, SubnetSummary};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/subnets", get(list_subnets).post(create_subnet))
        // 靜態路徑與既有 `/subnets/{id}` 動態路由並存，axum 以靜態優先
        // （同 `/assets/import` 前例）。
        .route("/subnets/export", get(export_subnets))
        .route(
            "/subnets/{id}",
            get(get_subnet).patch(update_subnet).delete(delete_subnet),
        )
}

/// 網段清單回應；`items` 為列表摘要（含已用／總數／衝突數，見票 07）。
#[derive(Debug, Serialize)]
struct SubnetItems {
    items: Vec<SubnetSummary>,
}

async fn list_subnets(State(state): State<AppState>) -> Result<Json<SubnetItems>, ApiError> {
    let items = subnets::list(&state.db).await?;

    Ok(Json(SubnetItems { items }))
}

/// 匯出全部網段 CSV（見 ADR-0009、spec §5）：attachment、UTF-8 BOM、
/// 檔名 `網段匯出_YYYYMMDD.csv`（RFC 5987 `filename*`）。
async fn export_subnets(State(state): State<AppState>) -> Result<Response<Body>, ApiError> {
    let subnets = subnets::list_full(&state.db)
        .await
        .map_err(|error| ApiError::internal("讀取網段清單失敗", error))?;
    let body = subnets::export_csv(&subnets)?;

    let date = Local::now().format("%Y%m%d");
    let filename = format!("網段匯出_{date}.csv");
    let content_disposition = format!(
        "attachment; filename=\"subnets_export_{date}.csv\"; filename*=UTF-8''{}",
        encode_filename(&filename)
    );

    Response::builder()
        .header(header::CONTENT_TYPE, "text/csv; charset=utf-8")
        .header(header::CONTENT_DISPOSITION, content_disposition)
        .body(Body::from(body))
        .map_err(|error| ApiError::internal("建立網段匯出回應失敗", error))
}

async fn get_subnet(
    State(state): State<AppState>,
    id: Result<Path<i64>, PathRejection>,
) -> Result<Json<Subnet>, ApiError> {
    let Path(id) = id.map_err(|_| ApiError::validation("網段 id 格式錯誤"))?;

    let subnet = subnets::get(&state.db, id)
        .await
        .map_err(|error| ApiError::internal("讀取網段失敗", error))?
        .ok_or_else(|| ApiError::not_found("找不到網段"))?;

    Ok(Json(subnet))
}

async fn create_subnet(
    State(state): State<AppState>,
    payload: Result<Json<SubnetInput>, JsonRejection>,
) -> Result<(StatusCode, Json<Subnet>), ApiError> {
    let Json(input) = payload.map_err(|_| ApiError::validation("請求內容格式錯誤"))?;
    let valid = input.validate()?;

    subnets::ensure_no_conflicts(&state.db, &valid, None).await?;

    let subnet = subnets::create(&state.db, valid)
        .await
        .map_err(|error| ApiError::internal("新增網段失敗", error))?;

    Ok((StatusCode::CREATED, Json(subnet)))
}

async fn update_subnet(
    State(state): State<AppState>,
    id: Result<Path<i64>, PathRejection>,
    payload: Result<Json<SubnetPatch>, JsonRejection>,
) -> Result<Json<Subnet>, ApiError> {
    let Path(id) = id.map_err(|_| ApiError::validation("網段 id 格式錯誤"))?;
    let Json(patch) = payload.map_err(|_| ApiError::validation("請求內容格式錯誤"))?;

    let existing = subnets::get(&state.db, id)
        .await
        .map_err(|error| ApiError::internal("讀取網段失敗", error))?
        .ok_or_else(|| ApiError::not_found("找不到網段"))?;

    let valid = patch.apply_to(&existing)?;
    subnets::ensure_no_conflicts(&state.db, &valid, Some(id)).await?;

    subnets::update(&state.db, id, valid)
        .await
        .map_err(|error| ApiError::internal("更新網段失敗", error))?
        .map(Json)
        .ok_or_else(|| ApiError::not_found("找不到網段"))
}

async fn delete_subnet(
    State(state): State<AppState>,
    id: Result<Path<i64>, PathRejection>,
) -> Result<StatusCode, ApiError> {
    let Path(id) = id.map_err(|_| ApiError::validation("網段 id 格式錯誤"))?;

    let subnet = subnets::get(&state.db, id)
        .await
        .map_err(|error| ApiError::internal("讀取網段失敗", error))?
        .ok_or_else(|| ApiError::not_found("找不到網段"))?;

    // 非空網段不可刪除（含保留與 v6 登錄；見 spec §2.3、票 08）。
    subnets::ensure_deletable(&state.db, &subnet).await?;

    let deleted = subnets::delete(&state.db, id)
        .await
        .map_err(|error| ApiError::internal("刪除網段失敗", error))?;

    if deleted {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::not_found("找不到網段"))
    }
}
