//! `/api/v1` 資產管理路由（見 spec §4.1、§5）。

use axum::body::Body;
use axum::extract::rejection::{JsonRejection, PathRejection, QueryRejection};
use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::Response;
use axum::routing::get;
use axum::{Json, Router};
use chrono::Local;
use serde::{Deserialize, Serialize};

use crate::AppState;
use crate::api::{ApiError, encode_filename};
use crate::asset_export;
use crate::assets::{
    self, Asset, AssetFilter, AssetInput, AssetPatch, SortDir, SortField, optional_text,
};
use crate::assignments::{self, AssetAssignment};
use crate::interfaces::{self, Interface};

/// 清單預設每頁筆數（見 spec §6）。
const DEFAULT_PER_PAGE: i64 = 50;
/// 每頁筆數上限，避免單次拉取過量資料。
const MAX_PER_PAGE: i64 = 200;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/assets", get(list_assets).post(create_asset))
        // 靜態路徑與既有 `/assets/{id}` 動態路由並存，axum 以靜態優先
        // （同 `/assets/import` 前例；見票 04）。
        .route("/assets/export", get(export_assets))
        .route(
            "/assets/{id}",
            get(get_asset).patch(update_asset).delete(delete_asset),
        )
        .route("/locations", get(list_locations))
        .route("/brands", get(list_brands))
        .route("/tags", get(list_tags))
}

#[derive(Debug, Deserialize)]
struct ListQuery {
    q: Option<String>,
    location: Option<String>,
    brand: Option<String>,
    device_serial: Option<String>,
    /// 標籤：不分大小寫完全符合（見票 11）。
    tag: Option<String>,
    /// 排序欄位白名單；無效值回 400。
    sort: Option<String>,
    /// 排序方向 `asc`／`desc`；無效值回 400。
    dir: Option<String>,
    page: Option<i64>,
    per_page: Option<i64>,
}

impl ListQuery {
    /// 驗證排序白名單並組成篩選條件；分頁供清單使用，匯出忽略
    /// （僅沿用其篩選與排序；見票 04）。
    fn into_filter(self, page: i64, per_page: i64) -> Result<AssetFilter, ApiError> {
        let sort = match optional_text(self.sort) {
            Some(value) => SortField::parse(&value).ok_or_else(|| {
                ApiError::validation(format!("無效的排序欄位：{value}")).field("sort")
            })?,
            None => SortField::default(),
        };
        let dir = match optional_text(self.dir) {
            Some(value) => SortDir::parse(&value).ok_or_else(|| {
                ApiError::validation(format!("無效的排序方向：{value}（僅接受 asc／desc）"))
                    .field("dir")
            })?,
            None => SortDir::default(),
        };

        Ok(AssetFilter {
            q: self.q,
            location: self.location,
            brand: self.brand,
            device_serial: self.device_serial,
            tag: self.tag,
            sort,
            dir,
            page,
            per_page,
        })
    }
}

/// 資產清單列：資產欄位攤平，加上全部已指派位址（「已指派 IP」欄；見票 12）
/// 與「最後可見」（見票 08）。
#[derive(Debug, Serialize)]
struct AssetListRow {
    #[serde(flatten)]
    asset: Asset,
    /// 已指派位址：跨介面、跨網段；v4 先、v6 後，同地址族依位址數值（見 spec §2.1）。
    assigned_ips: Vec<String>,
    /// 最後可見：介面指派位址或介面 MAC 命中的現況最大值；無命中為 `null`（見票 08）。
    last_seen_at: Option<String>,
}

#[derive(Debug, Serialize)]
struct AssetPage {
    items: Vec<AssetListRow>,
    total: i64,
    page: i64,
    per_page: i64,
}

#[derive(Debug, Serialize)]
struct StringItems {
    items: Vec<String>,
}

/// 資產詳情：資產欄位攤平，加上介面清單、已指派 IP 與最後可見
/// （唯讀顯示；見 spec §5、票 08）。
#[derive(Debug, Serialize)]
struct AssetDetail {
    #[serde(flatten)]
    asset: Asset,
    interfaces: Vec<Interface>,
    assignments: Vec<AssetAssignment>,
    /// 最後可見：無命中現況為 `null`（見票 08）。
    last_seen_at: Option<String>,
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
    let filter = query.into_filter(page, per_page)?;

    let (items, total) = assets::list(&state.db, &filter)
        .await
        .map_err(|error| ApiError::internal("讀取資產清單失敗", error))?;

    // 以各一筆查詢取當頁資產的已指派位址與最後可見，附入每列
    // （見票 12、票 08；不以逐資產查詢避免 N+1）。
    let ids: Vec<i64> = items.iter().map(|asset| asset.id).collect();
    let mut assigned = assignments::list_for_assets(&state.db, &ids)
        .await
        .map_err(|error| ApiError::internal("讀取已指派 IP 失敗", error))?;
    let mut last_seen = assets::last_seen_for_assets(&state.db, &ids)
        .await
        .map_err(|error| ApiError::internal("讀取資產最後可見失敗", error))?;
    let items = items
        .into_iter()
        .map(|asset| AssetListRow {
            assigned_ips: assigned.remove(&asset.id).unwrap_or_default(),
            last_seen_at: last_seen.remove(&asset.id),
            asset,
        })
        .collect();

    Ok(Json(AssetPage {
        items,
        total,
        page,
        per_page,
    }))
}

/// 匯出資產 CSV（見 spec §4、§5）：套用目前搜尋／篩選／排序的**全部**符合
/// 資產（忽略分頁）；attachment、UTF-8 BOM、檔名 `資產匯出_YYYYMMDD.csv`
/// （RFC 5987 `filename*`，比照票 01）。
async fn export_assets(
    State(state): State<AppState>,
    query: Result<Query<ListQuery>, QueryRejection>,
) -> Result<Response<Body>, ApiError> {
    let Query(query) = query.map_err(|_| ApiError::validation("查詢參數格式錯誤"))?;
    let filter = query.into_filter(1, DEFAULT_PER_PAGE)?;

    let body = asset_export::export_csv(&state.db, &filter).await?;

    let date = Local::now().format("%Y%m%d");
    let filename = format!("資產匯出_{date}.csv");
    let content_disposition = format!(
        "attachment; filename=\"assets_export_{date}.csv\"; filename*=UTF-8''{}",
        encode_filename(&filename)
    );

    Response::builder()
        .header(header::CONTENT_TYPE, "text/csv; charset=utf-8")
        .header(header::CONTENT_DISPOSITION, content_disposition)
        .body(Body::from(body))
        .map_err(|error| ApiError::internal("建立資產匯出回應失敗", error))
}

async fn get_asset(
    State(state): State<AppState>,
    id: Result<Path<i64>, PathRejection>,
) -> Result<Json<AssetDetail>, ApiError> {
    let Path(id) = id.map_err(|_| ApiError::validation("資產 id 格式錯誤"))?;

    let asset = assets::get(&state.db, id)
        .await
        .map_err(|error| ApiError::internal("讀取資產失敗", error))?
        .ok_or_else(|| ApiError::not_found("找不到資產"))?;

    let interfaces = interfaces::list_for_asset(&state.db, id)
        .await
        .map_err(|error| ApiError::internal("讀取介面清單失敗", error))?;

    let assignments = assignments::list_for_asset(&state.db, id)
        .await
        .map_err(|error| ApiError::internal("讀取已指派 IP 失敗", error))?;

    let last_seen_at = assets::last_seen_for_asset(&state.db, id)
        .await
        .map_err(|error| ApiError::internal("讀取資產最後可見失敗", error))?;

    Ok(Json(AssetDetail {
        asset,
        interfaces,
        assignments,
        last_seen_at,
    }))
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

/// 標籤建議值：所有已使用標籤去重（不分大小寫；見票 11）。
async fn list_tags(State(state): State<AppState>) -> Result<Json<StringItems>, ApiError> {
    let items = assets::tags(&state.db)
        .await
        .map_err(|error| ApiError::internal("讀取標籤清單失敗", error))?;
    Ok(Json(StringItems { items }))
}
