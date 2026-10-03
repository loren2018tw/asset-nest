//! `/api/v1` IP 位址路由（見 spec §4.3、§5）。
//!
//! v4 提供清單與指派／改用途、取消指派；v6（票 06）為登錄制：
//! `POST /subnets/{id}/ips` 新增即指派，清單僅列登錄位址，指派端點限 static。
//! 衝突標記（票 07）後續擴充。

use std::net::IpAddr;

use axum::extract::rejection::{JsonRejection, PathRejection, QueryRejection};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, put};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use crate::AppState;
use crate::api::ApiError;
use crate::assignments::{self, Assignment, AssignmentInput, RegisterInput};
use crate::ips::{self, IpEntry, IpFilter, IpStatusFilter};
use crate::subnets;

/// 清單預設每頁筆數（見 spec §6）；上限比照 `/assets`。
const DEFAULT_PER_PAGE: i64 = 50;
const MAX_PER_PAGE: i64 = 200;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/subnets/{id}/ips",
            get(list_subnet_ips).post(post_subnet_ip),
        )
        .route(
            "/subnets/{id}/ips/{address}/assignment",
            put(put_assignment).delete(delete_assignment),
        )
}

#[derive(Debug, Deserialize)]
struct ListQuery {
    q: Option<String>,
    status: Option<String>,
    page: Option<i64>,
    per_page: Option<i64>,
}

/// IP 清單回應；`total` 為符合條件的位址總數（伺服器端分頁）。
#[derive(Debug, Serialize)]
struct IpPage {
    items: Vec<IpEntry>,
    total: u64,
    page: i64,
    per_page: i64,
}

async fn list_subnet_ips(
    State(state): State<AppState>,
    id: Result<Path<i64>, PathRejection>,
    query: Result<Query<ListQuery>, QueryRejection>,
) -> Result<Json<IpPage>, ApiError> {
    let Path(id) = id.map_err(|_| ApiError::validation("網段 id 格式錯誤"))?;
    let Query(query) = query.map_err(|_| ApiError::validation("查詢參數格式錯誤"))?;

    let subnet = subnets::get(&state.db, id)
        .await
        .map_err(|error| ApiError::internal("讀取網段失敗", error))?
        .ok_or_else(|| ApiError::not_found("找不到網段"))?;

    let status = match query
        .status
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(value) => Some(IpStatusFilter::parse(value).ok_or_else(|| {
            ApiError::validation("狀態篩選須為 available、in_pool、static 或 reservation")
                .field("status")
        })?),
        None => None,
    };

    let page = query.page.unwrap_or(1).max(1);
    let per_page = query
        .per_page
        .unwrap_or(DEFAULT_PER_PAGE)
        .clamp(1, MAX_PER_PAGE);

    let filter = IpFilter {
        q: query.q,
        status,
        page,
        per_page,
    };

    let assignments = assignments::list_for_subnet(&state.db, id)
        .await
        .map_err(|error| ApiError::internal("讀取指派清單失敗", error))?;

    // v4 枚舉全部 host；v6 僅列出已登錄（有指派）位址（見票 06）。
    let (items, total) = ips::list(&subnet, &filter, &assignments)?;

    Ok(Json(IpPage {
        items,
        total,
        page,
        per_page,
    }))
}

/// v6 登錄位址（新增即指派；用途固定 static）；v4 網段回 400。
async fn post_subnet_ip(
    State(state): State<AppState>,
    id: Result<Path<i64>, PathRejection>,
    payload: Result<Json<RegisterInput>, JsonRejection>,
) -> Result<(StatusCode, Json<Assignment>), ApiError> {
    let Path(id) = id.map_err(|_| ApiError::validation("網段 id 格式錯誤"))?;
    let Json(input) = payload.map_err(|_| ApiError::validation("請求內容格式錯誤"))?;

    let subnet = subnets::get(&state.db, id)
        .await
        .map_err(|error| ApiError::internal("讀取網段失敗", error))?
        .ok_or_else(|| ApiError::not_found("找不到網段"))?;

    let (address, interface_id) = input.validate()?;
    let assignment = assignments::register(&state.db, &subnet, address, interface_id).await?;

    Ok((StatusCode::CREATED, Json(assignment)))
}

/// 指派或改用途（含 hostname）；結構錯誤回 400＋明確 `details`。
async fn put_assignment(
    State(state): State<AppState>,
    path: Result<Path<(i64, String)>, PathRejection>,
    payload: Result<Json<AssignmentInput>, JsonRejection>,
) -> Result<Json<Assignment>, ApiError> {
    let Path((id, address)) = path.map_err(|_| ApiError::validation("路徑參數格式錯誤"))?;
    let Json(input) = payload.map_err(|_| ApiError::validation("請求內容格式錯誤"))?;

    let subnet = subnets::get(&state.db, id)
        .await
        .map_err(|error| ApiError::internal("讀取網段失敗", error))?
        .ok_or_else(|| ApiError::not_found("找不到網段"))?;

    let address = parse_address(&address)?;
    let valid = input.validate()?;

    let assignment = assignments::assign(&state.db, &subnet, address, valid).await?;

    Ok(Json(assignment))
}

/// 取消指派；不存在回 404。
async fn delete_assignment(
    State(state): State<AppState>,
    path: Result<Path<(i64, String)>, PathRejection>,
) -> Result<StatusCode, ApiError> {
    let Path((id, address)) = path.map_err(|_| ApiError::validation("路徑參數格式錯誤"))?;

    if subnets::get(&state.db, id)
        .await
        .map_err(|error| ApiError::internal("讀取網段失敗", error))?
        .is_none()
    {
        return Err(ApiError::not_found("找不到網段"));
    }

    let address = parse_address(&address)?;

    let cancelled = assignments::cancel(&state.db, id, address)
        .await
        .map_err(|error| ApiError::internal("取消指派失敗", error))?;

    if cancelled {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::not_found("找不到指派"))
    }
}

/// 解析路徑中的位址（v4／v6 皆可）；與網段的地址族是否相符由領域層檢查。
fn parse_address(text: &str) -> Result<IpAddr, ApiError> {
    text.parse()
        .map_err(|_| ApiError::validation(format!("位址格式錯誤：{text}")).field("address"))
}
