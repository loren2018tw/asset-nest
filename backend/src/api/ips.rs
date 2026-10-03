//! `/api/v1` IP 位址路由（見 spec §4.3、§5）。
//!
//! 本票（04）僅提供 v4 唯讀清單；v6（票 06）、指派（票 05）、
//! 衝突標記（票 07）後續擴充。

use axum::extract::rejection::{PathRejection, QueryRejection};
use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use crate::AppState;
use crate::api::ApiError;
use crate::ips::{self, IpEntry, IpFilter};
use crate::subnets;

/// 清單預設每頁筆數（見 spec §6）；上限比照 `/assets`。
const DEFAULT_PER_PAGE: i64 = 50;
const MAX_PER_PAGE: i64 = 200;

pub fn router() -> Router<AppState> {
    Router::new().route("/subnets/{id}/ips", get(list_subnet_ips))
}

#[derive(Debug, Deserialize)]
struct ListQuery {
    q: Option<String>,
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

    let page = query.page.unwrap_or(1).max(1);
    let per_page = query
        .per_page
        .unwrap_or(DEFAULT_PER_PAGE)
        .clamp(1, MAX_PER_PAGE);

    let filter = IpFilter {
        q: query.q,
        page,
        per_page,
    };

    // v6 網段由 `ips::list` 回傳結構化的 `not_implemented`（見票 06）。
    let (items, total) = ips::list(&subnet, &filter)?;

    Ok(Json(IpPage {
        items,
        total,
        page,
        per_page,
    }))
}
