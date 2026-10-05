//! `/api/v1` IP 位址路由（見 spec §4.3、§5）。
//!
//! v4 提供清單與指派／改用途、取消指派；v6（票 06）為登錄制：
//! `POST /subnets/{id}/ips` 新增即指派，清單僅列登錄位址，指派端點限 static。
//! 衝突標記（票 07）由 `GET` 列徽章與儲存回應的 `warnings` 呈現（見 ADR-0006）。
//! 受管網段的保留指派／取消會即時推送 Kea，回應附 `kea_sync`（見 ADR-0011）。

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
use crate::conflicts;
use crate::interfaces::Warning;
use crate::ips::{self, IpEntry, IpFilter, IpSortDir, IpSortField, IpStatusFilter};
use crate::kea::sync as kea_sync;
use crate::kea::sync::KeaSync;
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
    /// 排序欄位白名單（`address`／`status`／`location`／`assignment`）；
    /// 無效值回 400（見票 14）。
    sort: Option<String>,
    /// 排序方向 `asc`／`desc`；無效值回 400。
    dir: Option<String>,
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

/// 指派儲存回應：指派欄位攤平，加上不阻擋的語意警示（見 ADR-0006）與 Kea 推送結果。
#[derive(Debug, Serialize)]
struct AssignmentResponse {
    #[serde(flatten)]
    assignment: Assignment,
    warnings: Vec<Warning>,
    /// Kea 單筆推送結果；僅在應同步時出現（見 `docs/adr/0011`）。
    #[serde(skip_serializing_if = "Option::is_none")]
    kea_sync: Option<KeaSync>,
}

/// 取消指派回應：`kea_sync` 僅在應同步時出現。
#[derive(Debug, Serialize)]
struct AssignmentDeleted {
    #[serde(skip_serializing_if = "Option::is_none")]
    kea_sync: Option<KeaSync>,
}

/// 建立指派儲存回應：重新偵測該網段衝突並附上警示（僅提示、不阻擋）。
///
/// 每次儲存即時重算：新增保留可能使既有保留也命中 DuplicateHwAddress；
/// 取消指派後的下一次讀取亦同（見票 07）。
async fn respond_with_warnings(
    state: &AppState,
    subnet: &subnets::Subnet,
    assignment: Assignment,
    kea_sync: Option<KeaSync>,
) -> Result<Json<AssignmentResponse>, ApiError> {
    let warnings = warnings_for_address(state, subnet, &assignment.address).await?;
    Ok(Json(AssignmentResponse {
        assignment,
        warnings,
        kea_sync,
    }))
}

/// 重新偵測該網段衝突並回傳該位址的警示（僅提示、不阻擋）。
///
/// 供指派端點與資產端指派共用；每次儲存即時重算，無快取（見票 07）。
pub(super) async fn warnings_for_address(
    state: &AppState,
    subnet: &subnets::Subnet,
    address: &str,
) -> Result<Vec<Warning>, ApiError> {
    let listed = assignments::list_for_subnet(&state.db, subnet.id)
        .await
        .map_err(|error| ApiError::internal("讀取指派清單失敗", error))?;
    conflicts::warnings_for(subnet, &listed, address)
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

    // 排序欄位與方向經白名單驗證，無效值回 400（比照 `/assets`，見票 14）。
    let sort = match query
        .sort
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(value) => IpSortField::parse(value).ok_or_else(|| {
            ApiError::validation(format!("無效的排序欄位：{value}")).field("sort")
        })?,
        None => IpSortField::default(),
    };
    let dir = match query
        .dir
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(value) => IpSortDir::parse(value).ok_or_else(|| {
            ApiError::validation(format!("無效的排序方向：{value}（僅接受 asc／desc）"))
                .field("dir")
        })?,
        None => IpSortDir::default(),
    };

    let page = query.page.unwrap_or(1).max(1);
    let per_page = query
        .per_page
        .unwrap_or(DEFAULT_PER_PAGE)
        .clamp(1, MAX_PER_PAGE);

    let filter = IpFilter {
        q: query.q,
        status,
        sort,
        dir,
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
) -> Result<(StatusCode, Json<AssignmentResponse>), ApiError> {
    let Path(id) = id.map_err(|_| ApiError::validation("網段 id 格式錯誤"))?;
    let Json(input) = payload.map_err(|_| ApiError::validation("請求內容格式錯誤"))?;

    let subnet = subnets::get(&state.db, id)
        .await
        .map_err(|error| ApiError::internal("讀取網段失敗", error))?
        .ok_or_else(|| ApiError::not_found("找不到網段"))?;

    let (address, interface_id) = input.validate()?;
    let assignment = assignments::register(&state.db, &subnet, address, interface_id).await?;
    let response = respond_with_warnings(&state, &subnet, assignment, None).await?;

    Ok((StatusCode::CREATED, response))
}

/// 指派或改用途（含 hostname）；結構錯誤回 400＋明確 `details`。
///
/// 受管網段的保留指派即時推送 Kea（見 ADR-0011）。
async fn put_assignment(
    State(state): State<AppState>,
    path: Result<Path<(i64, String)>, PathRejection>,
    payload: Result<Json<AssignmentInput>, JsonRejection>,
) -> Result<Json<AssignmentResponse>, ApiError> {
    let Path((id, address)) = path.map_err(|_| ApiError::validation("路徑參數格式錯誤"))?;
    let Json(input) = payload.map_err(|_| ApiError::validation("請求內容格式錯誤"))?;

    let subnet = subnets::get(&state.db, id)
        .await
        .map_err(|error| ApiError::internal("讀取網段失敗", error))?
        .ok_or_else(|| ApiError::not_found("找不到網段"))?;

    let address = parse_address(&address)?;
    let valid = input.validate()?;

    // 同步判斷需要異動前狀態（保留→static 時刪除 Kea 保留）。
    let before = assignments::find(&state.db, subnet.id, &address.to_string())
        .await
        .map_err(|error| ApiError::internal("讀取指派失敗", error))?;

    let assignment = assignments::assign(&state.db, &subnet, address, valid).await?;
    let kea_sync = kea_sync::after_assignment_change(
        &state.db,
        state.kea.as_ref(),
        &subnet,
        before.as_ref(),
        Some(&assignment),
    )
    .await?;

    respond_with_warnings(&state, &subnet, assignment, kea_sync).await
}

/// 取消指派；不存在回 404。回應 200；`kea_sync` 僅在應同步時出現（見 ADR-0011）。
///
/// 其他列的衝突（如 DuplicateHwAddress）於下一次讀取時即時重算、自然消失（見票 07）。
async fn delete_assignment(
    State(state): State<AppState>,
    path: Result<Path<(i64, String)>, PathRejection>,
) -> Result<Json<AssignmentDeleted>, ApiError> {
    let Path((id, address)) = path.map_err(|_| ApiError::validation("路徑參數格式錯誤"))?;

    let subnet = subnets::get(&state.db, id)
        .await
        .map_err(|error| ApiError::internal("讀取網段失敗", error))?
        .ok_or_else(|| ApiError::not_found("找不到網段"))?;

    let address = parse_address(&address)?;

    // 同步判斷需要異動前狀態（取消保留時刪除 Kea 保留）。
    let before = assignments::find(&state.db, subnet.id, &address.to_string())
        .await
        .map_err(|error| ApiError::internal("讀取指派失敗", error))?;

    let cancelled = assignments::cancel(&state.db, id, address)
        .await
        .map_err(|error| ApiError::internal("取消指派失敗", error))?;

    if !cancelled {
        return Err(ApiError::not_found("找不到指派"));
    }

    let kea_sync = kea_sync::after_assignment_change(
        &state.db,
        state.kea.as_ref(),
        &subnet,
        before.as_ref(),
        None,
    )
    .await?;

    Ok(Json(AssignmentDeleted { kea_sync }))
}

/// 解析路徑中的位址（v4／v6 皆可）；與網段的地址族是否相符由領域層檢查。
fn parse_address(text: &str) -> Result<IpAddr, ApiError> {
    text.parse()
        .map_err(|_| ApiError::validation(format!("位址格式錯誤：{text}")).field("address"))
}
