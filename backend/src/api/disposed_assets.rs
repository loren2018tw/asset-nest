//! `/api/v1` 資產報廢路由（見 `.scratch/asset-disposal/spec.md` §4、`docs/adr/0026`）。
//!
//! - `POST /assets/{id}/disposal`：驗證（400）→ 資產不存在（404）→ 出借中
//!   （409）→ 先擷取保留、本地搬移、後 Kea 推送（保留逐筆刪除；失敗不阻擋；
//!   見 ADR-0027）；201＋攤平報廢欄位＋`kea_sync`。
//! - `GET /disposed-assets?q=&page=&per_page=`：報廢清單，伺服器分頁。
//! - `POST /disposed-assets/{id}/restore`：復原；不存在（404）→ 200＋`Asset`。

use axum::extract::rejection::{JsonRejection, PathRejection, QueryRejection};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::Local;
use serde::{Deserialize, Serialize};

use crate::AppState;
use crate::api::ApiError;
use crate::assets::{self, Asset};
use crate::disposed_assets::{self, DisposeInput, DisposedAsset};
use crate::kea::push as kea_push;
use crate::kea::push::KeaSync;
use crate::lendings;

/// 報廢清單預設每頁筆數（見 spec §4）。
const DEFAULT_PER_PAGE: i64 = 10;
/// 每頁筆數上限，避免單次拉取過量資料（比照 assets 的 `MAX_PER_PAGE`）。
const MAX_PER_PAGE: i64 = 100;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/assets/{id}/disposal", post(dispose_asset))
        .route("/disposed-assets", get(list_disposed_assets))
        .route(
            "/disposed-assets/{id}/restore",
            post(restore_disposed_asset),
        )
}

/// `GET /disposed-assets` 查詢參數。
#[derive(Debug, Deserialize)]
struct ListQuery {
    q: Option<String>,
    page: Option<i64>,
    per_page: Option<i64>,
}

/// `GET /disposed-assets` 回應（見 spec §4）。
#[derive(Debug, Serialize)]
struct DisposedAssetPage {
    items: Vec<DisposedAsset>,
    total: i64,
    page: i64,
    per_page: i64,
}

/// 報廢回應：報廢欄位攤平＋Kea 推送結果（僅涉及同步的保留逐筆；見 spec §4）。
#[derive(Debug, Serialize)]
struct DisposeResponse {
    #[serde(flatten)]
    disposed: DisposedAsset,
    kea_sync: Vec<KeaSync>,
}

/// 報廢：驗證（400）→ 資產存在（404）→ 出借中（409）→ 搬移 → Kea 推送
/// （見 spec §4、ADR-0027 的順序修訂）。
async fn dispose_asset(
    State(state): State<AppState>,
    id: Result<Path<i64>, PathRejection>,
    payload: Result<Json<DisposeInput>, JsonRejection>,
) -> Result<(StatusCode, Json<DisposeResponse>), ApiError> {
    let Path(asset_id) = id.map_err(|_| ApiError::validation("資產 id 格式錯誤"))?;
    let Json(input) = payload.map_err(|_| ApiError::validation("請求內容格式錯誤"))?;
    let valid = input.validate(Local::now().date_naive())?;

    assets::get(&state.db, asset_id)
        .await
        .map_err(|error| ApiError::internal("讀取資產失敗", error))?
        .ok_or_else(|| ApiError::not_found("找不到資產"))?;

    if lendings::has_open_lending(&state.db, asset_id)
        .await
        .map_err(|error| ApiError::internal("讀取借出紀錄失敗", error))?
    {
        return Err(ApiError::conflict("此資產出借中，請先歸還"));
    }

    // 先擷取保留（搬移後指派連動消失），再搬移、後推送 Kea（見 ADR-0027）；
    // 捕捉必在本地異動前，搬移失敗回 404、不推送。
    let pending = kea_push::capture_asset_deletion(&state.db, asset_id).await?;

    let disposed = disposed_assets::dispose(&state.db, asset_id, valid)
        .await
        .map_err(|error| ApiError::internal("報廢資產失敗", error))?
        .ok_or_else(|| ApiError::not_found("找不到資產"))?;

    let kea_sync = kea_push::commit_deletions(&state.db, state.kea.as_ref(), pending).await?;

    Ok((
        StatusCode::CREATED,
        Json(DisposeResponse { disposed, kea_sync }),
    ))
}

/// 報廢清單：`q` 關鍵字搜尋、伺服器分頁（`page` 由 1、`per_page` 預設 10 夾
/// 1..=100；見 spec §4）。
async fn list_disposed_assets(
    State(state): State<AppState>,
    query: Result<Query<ListQuery>, QueryRejection>,
) -> Result<Json<DisposedAssetPage>, ApiError> {
    let Query(query) = query.map_err(|_| ApiError::validation("查詢參數格式錯誤"))?;

    let page = query.page.unwrap_or(1).max(1);
    let per_page = query
        .per_page
        .unwrap_or(DEFAULT_PER_PAGE)
        .clamp(1, MAX_PER_PAGE);

    let (items, total) = disposed_assets::list(&state.db, query.q.as_deref(), page, per_page)
        .await
        .map_err(|error| ApiError::internal("讀取報廢清單失敗", error))?;

    Ok(Json(DisposedAssetPage {
        items,
        total,
        page,
        per_page,
    }))
}

/// 復原：搬回 `assets`（原 id、原 `created_at`）；報廢列不存在回 404
/// （見 spec §4）。
async fn restore_disposed_asset(
    State(state): State<AppState>,
    id: Result<Path<i64>, PathRejection>,
) -> Result<Json<Asset>, ApiError> {
    let Path(id) = id.map_err(|_| ApiError::validation("報廢資產 id 格式錯誤"))?;

    disposed_assets::restore(&state.db, id)
        .await
        .map_err(|error| ApiError::internal("復原資產失敗", error))?
        .map(Json)
        .ok_or_else(|| ApiError::not_found("找不到報廢資產"))
}
