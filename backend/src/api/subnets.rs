//! `/api/v1` 網段設定路由（見 spec §4.2、§5）。

use axum::body::Body;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{Local, Utc};
use serde::{Deserialize, Serialize};

use crate::AppState;
use crate::api::{ApiError, encode_filename};
use crate::observation::{self, SweepReport};
use crate::probe::Prober;
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
        // 手動觸發掃描（票 02 僅快速掃描；探索掃描為票 05）。
        .route("/subnets/{id}/sweeps", post(sweep_subnet))
}

/// 網段清單回應；`items` 為列表摘要（含已用／總數／衝突數與觀測欄位，見票 07、票 01）。
#[derive(Debug, Serialize)]
struct SubnetItems {
    items: Vec<SubnetSummary>,
}

/// 網段詳情回應：網段欄位＋本機同 L2 判定（見票 01）。
#[derive(Debug, Serialize)]
struct SubnetDetail {
    #[serde(flatten)]
    subnet: Subnet,
    /// `prober.is_local`：本機是否有介面位址落在該 v4 子網；v6 恆為 false。
    local: bool,
}

impl SubnetDetail {
    fn new(subnet: Subnet, prober: &dyn Prober) -> Self {
        Self {
            local: prober.is_local(&subnet),
            subnet,
        }
    }
}

async fn list_subnets(State(state): State<AppState>) -> Result<Json<SubnetItems>, ApiError> {
    let items = subnets::list(&state.db, state.prober.as_ref()).await?;

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
) -> Result<Json<SubnetDetail>, ApiError> {
    let Path(id) = id.map_err(|_| ApiError::validation("網段 id 格式錯誤"))?;

    let subnet = subnets::get(&state.db, id)
        .await
        .map_err(|error| ApiError::internal("讀取網段失敗", error))?
        .ok_or_else(|| ApiError::not_found("找不到網段"))?;

    Ok(Json(SubnetDetail::new(subnet, state.prober.as_ref())))
}

async fn create_subnet(
    State(state): State<AppState>,
    payload: Result<Json<SubnetInput>, JsonRejection>,
) -> Result<(StatusCode, Json<SubnetDetail>), ApiError> {
    let Json(input) = payload.map_err(|_| ApiError::validation("請求內容格式錯誤"))?;
    let valid = input.validate()?;

    subnets::ensure_no_conflicts(&state.db, &valid, None).await?;

    let subnet = subnets::create(&state.db, valid)
        .await
        .map_err(|error| ApiError::internal("新增網段失敗", error))?;

    Ok((
        StatusCode::CREATED,
        Json(SubnetDetail::new(subnet, state.prober.as_ref())),
    ))
}

async fn update_subnet(
    State(state): State<AppState>,
    id: Result<Path<i64>, PathRejection>,
    payload: Result<Json<SubnetPatch>, JsonRejection>,
) -> Result<Json<SubnetDetail>, ApiError> {
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
        .map(|subnet| Json(SubnetDetail::new(subnet, state.prober.as_ref())))
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

/// 掃描輸入；`mode` 目前僅支援 `quick`（`discovery` 為票 05）。
#[derive(Debug, Deserialize)]
struct SweepInput {
    mode: Option<String>,
}

/// `POST /subnets/{id}/sweeps`：同步執行掃描並回摘要（見票 02、spec §HTTP API）。
///
/// 前提由 [`observation::run_quick`] 驗證：v4、已開觀測、本機同 L2，
/// 否則回 400 明確訊息；未知模式與尚未實作的 `discovery` 亦回 400。
async fn sweep_subnet(
    State(state): State<AppState>,
    id: Result<Path<i64>, PathRejection>,
    payload: Result<Json<SweepInput>, JsonRejection>,
) -> Result<Json<SweepReport>, ApiError> {
    let Path(id) = id.map_err(|_| ApiError::validation("網段 id 格式錯誤"))?;
    let Json(input) = payload.map_err(|_| ApiError::validation("請求內容格式錯誤"))?;

    match input.mode.as_deref().map(str::trim).unwrap_or_default() {
        "quick" => {}
        "discovery" => {
            return Err(ApiError::validation("探索掃描尚未支援（見票 05）").field("mode"));
        }
        "" => {
            return Err(ApiError::validation("mode 為必填（目前僅支援 quick）").field("mode"));
        }
        other => {
            return Err(
                ApiError::validation(format!("不支援的掃描模式：{other}（僅支援 quick）"))
                    .field("mode"),
            );
        }
    }

    let subnet = subnets::get(&state.db, id)
        .await
        .map_err(|error| ApiError::internal("讀取網段失敗", error))?
        .ok_or_else(|| ApiError::not_found("找不到網段"))?;

    let report = observation::run_quick(
        &state.db,
        state.prober.clone(),
        state.kea.as_ref(),
        &subnet,
        Utc::now(),
    )
    .await?;
    Ok(Json(report))
}
