//! `/api/v1` 借還路由（見 `.scratch/asset-lending/spec.md` §4）。
//!
//! - `POST /assets/{id}/lendings`：建立借出（資產不存在 → 404；已有未歸還
//!   借出 → 409「此資產已在出借中」；驗證錯誤 → 400）。
//! - `POST /lendings/{id}/return`：歸還（不存在 → 404；已歸還 → 409）。
//! - `GET /lendings?returned=false`：出借中清單，不分頁。
//! - `GET /lendings?returned=true&page=&per_page=`：已歸還紀錄，分頁。
//! - `GET /lendings/borrowers`：借用人建議值（最近使用者在前）。

use axum::extract::rejection::{JsonRejection, PathRejection, QueryRejection};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::Local;
use serde::{Deserialize, Serialize};

use crate::AppState;
use crate::api::ApiError;
use crate::assets;
use crate::lendings::{self, Lending, LendingInput, LendingWithAsset, ReturnOutcome};

/// 已歸還紀錄預設每頁筆數（見 spec §4）。
const DEFAULT_PER_PAGE: i64 = 10;
/// 每頁筆數上限，避免單次拉取過量資料（比照 assets 的 `MAX_PER_PAGE`）。
const MAX_PER_PAGE: i64 = 100;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/assets/{id}/lendings", post(create_lending))
        .route("/lendings/{id}/return", post(return_lending))
        .route("/lendings", get(list_lendings))
        .route("/lendings/borrowers", get(list_borrowers))
}

/// `GET /lendings` 查詢參數；`returned` 缺漏視為 `false`（出借中）。
#[derive(Debug, Deserialize)]
struct LendingsQuery {
    returned: Option<bool>,
    page: Option<i64>,
    per_page: Option<i64>,
}

/// `GET /lendings` 回應：出借中僅含 `items`；已歸還另附分頁欄位
/// （未用到的欄位不序列化，見 spec §4）。
#[derive(Debug, Serialize)]
struct LendingsList {
    items: Vec<LendingWithAsset>,
    #[serde(skip_serializing_if = "Option::is_none")]
    total: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    page: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    per_page: Option<i64>,
}

/// `GET /lendings/borrowers` 回應。
#[derive(Debug, Serialize)]
struct Borrowers {
    items: Vec<String>,
}

/// 出借中清單（`returned=false`，不分頁）或已歸還紀錄（`returned=true`，分頁）。
///
/// `returned=true` 時 `page` 由 1 起、`per_page` 預設 10 夾 1..=100
/// （見 spec §4）；兩者皆依借出時間倒序。
async fn list_lendings(
    State(state): State<AppState>,
    query: Result<Query<LendingsQuery>, QueryRejection>,
) -> Result<Json<LendingsList>, ApiError> {
    let Query(query) = query.map_err(|_| ApiError::validation("查詢參數格式錯誤"))?;
    let today = Local::now().date_naive();

    if query.returned.unwrap_or(false) {
        let page = query.page.unwrap_or(1).max(1);
        let per_page = query
            .per_page
            .unwrap_or(DEFAULT_PER_PAGE)
            .clamp(1, MAX_PER_PAGE);

        let (items, total) = lendings::list_returned(&state.db, page, per_page, today)
            .await
            .map_err(|error| ApiError::internal("讀取已歸還紀錄失敗", error))?;

        Ok(Json(LendingsList {
            items,
            total: Some(total),
            page: Some(page),
            per_page: Some(per_page),
        }))
    } else {
        let items = lendings::list_open(&state.db, today)
            .await
            .map_err(|error| ApiError::internal("讀取出借中清單失敗", error))?;

        Ok(Json(LendingsList {
            items,
            total: None,
            page: None,
            per_page: None,
        }))
    }
}

/// 建立借出：驗證輸入（400）→ 資產存在（404）→ 無未歸還借出（409）→ 寫入。
async fn create_lending(
    State(state): State<AppState>,
    id: Result<Path<i64>, PathRejection>,
    payload: Result<Json<LendingInput>, JsonRejection>,
) -> Result<(StatusCode, Json<Lending>), ApiError> {
    let Path(asset_id) = id.map_err(|_| ApiError::validation("資產 id 格式錯誤"))?;
    let Json(input) = payload.map_err(|_| ApiError::validation("請求內容格式錯誤"))?;
    let valid = input.validate()?;

    assets::get(&state.db, asset_id)
        .await
        .map_err(|error| ApiError::internal("讀取資產失敗", error))?
        .ok_or_else(|| ApiError::not_found("找不到資產"))?;

    if lendings::has_open_lending(&state.db, asset_id)
        .await
        .map_err(|error| ApiError::internal("讀取借出紀錄失敗", error))?
    {
        return Err(ApiError::conflict("此資產已在出借中"));
    }

    let lending = lendings::create(&state.db, asset_id, valid, Local::now().date_naive())
        .await
        .map_err(|error| ApiError::internal("新增借出失敗", error))?;

    Ok((StatusCode::CREATED, Json(lending)))
}

/// 歸還：以 [`ReturnOutcome`] 對映 200／404／409（見 spec §2 不變量）。
async fn return_lending(
    State(state): State<AppState>,
    id: Result<Path<i64>, PathRejection>,
) -> Result<Json<Lending>, ApiError> {
    let Path(id) = id.map_err(|_| ApiError::validation("借出紀錄 id 格式錯誤"))?;

    match lendings::return_one(&state.db, id, Local::now().date_naive())
        .await
        .map_err(|error| ApiError::internal("歸還失敗", error))?
    {
        ReturnOutcome::Returned(lending) => Ok(Json(lending)),
        ReturnOutcome::NotFound => Err(ApiError::not_found("找不到借出紀錄")),
        ReturnOutcome::AlreadyReturned => Err(ApiError::conflict("此借出紀錄已歸還")),
    }
}

/// 借用人建議值：既有借用人去重（不分大小寫），最近使用者在前。
async fn list_borrowers(State(state): State<AppState>) -> Result<Json<Borrowers>, ApiError> {
    let items = lendings::list_borrowers(&state.db)
        .await
        .map_err(|error| ApiError::internal("讀取借用人清單失敗", error))?;
    Ok(Json(Borrowers { items }))
}
