//! `/api/v1` 觀測歷史路由（見票 06、spec §HTTP API）。
//!
//! 唯讀端點：IP 歷史、MAC 歷史與單一 IP 歷史 CSV 匯出；宣告資料完全不變
//! （見 ADR-0014）。彙總規則見 [`crate::observation`]。

use std::net::IpAddr;

use axum::body::Body;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::http::header;
use axum::response::Response;
use axum::routing::get;
use axum::{Json, Router};
use chrono::Local;

use crate::AppState;
use crate::api::{ApiError, encode_filename};
use crate::observation::{self, IpHistory, MacHistory};
use crate::probe::normalize_mac;
use crate::subnets::{self, Subnet};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/subnets/{id}/ips/{address}/observations",
            get(get_ip_history),
        )
        .route(
            "/subnets/{id}/ips/{address}/observations/export",
            get(export_ip_history),
        )
        .route("/observations/mac/{mac}", get(get_mac_history))
}

/// 讀取網段內某位址的觀測歷史：現況＋事件（新到舊）＋用過的 MAC。
async fn get_ip_history(
    State(state): State<AppState>,
    path: Result<Path<(i64, String)>, PathRejection>,
) -> Result<Json<IpHistory>, ApiError> {
    let Path((id, address)) = path.map_err(|_| ApiError::validation("路徑參數格式錯誤"))?;
    let subnet = load_subnet(&state, id).await?;
    let address = validate_address(&address)?;

    let history =
        observation::ip_history(&state.db, state.prober.as_ref(), &subnet, &address).await?;
    Ok(Json(history))
}

/// 匯出網段內某位址的觀測歷史 CSV（見票 06、spec §HTTP API）：attachment、
/// UTF-8 BOM、檔名 `觀測歷史_<位址>_YYYYMMDD.csv`（RFC 5987 `filename*`，
/// 比照既有匯出）。資料列＝事件新到舊；無 JSON 形式（錯誤仍為 JSON）。
async fn export_ip_history(
    State(state): State<AppState>,
    path: Result<Path<(i64, String)>, PathRejection>,
) -> Result<Response<Body>, ApiError> {
    let Path((id, address)) = path.map_err(|_| ApiError::validation("路徑參數格式錯誤"))?;
    let subnet = load_subnet(&state, id).await?;
    let address = validate_address(&address)?;

    let history =
        observation::ip_history(&state.db, state.prober.as_ref(), &subnet, &address).await?;
    let body = observation::history_export_csv(&history.events)?;

    let date = Local::now().format("%Y%m%d");
    let filename = format!("觀測歷史_{address}_{date}.csv");
    let content_disposition = format!(
        "attachment; filename=\"observations_{address}_{date}.csv\"; filename*=UTF-8''{}",
        encode_filename(&filename)
    );

    Response::builder()
        .header(header::CONTENT_TYPE, "text/csv; charset=utf-8")
        .header(header::CONTENT_DISPOSITION, content_disposition)
        .body(Body::from(body))
        .map_err(|error| ApiError::internal("建立觀測歷史匯出回應失敗", error))
}

/// 讀取某 MAC 的觀測歷史：用過哪些位址（首見／最後可見／來源）＋已知資產連結。
///
/// MAC 先以 [`normalize_mac`] 正規化；格式不符回 400（見票 06）。
async fn get_mac_history(
    State(state): State<AppState>,
    mac: Result<Path<String>, PathRejection>,
) -> Result<Json<MacHistory>, ApiError> {
    let Path(mac) = mac.map_err(|_| ApiError::validation("路徑參數格式錯誤"))?;
    let Some(normalized) = normalize_mac(&mac) else {
        return Err(ApiError::validation(
            "MAC 格式錯誤：須為 6 或 8 組兩位十六進位（可用冒號或連字號分隔）",
        )
        .field("mac"));
    };

    let history = observation::mac_history(&state.db, &normalized).await?;
    Ok(Json(history))
}

/// 讀取網段；不存在回 404（歷史端點的共同前提）。
async fn load_subnet(state: &AppState, id: i64) -> Result<Subnet, ApiError> {
    subnets::get(&state.db, id)
        .await
        .map_err(|error| ApiError::internal("讀取網段失敗", error))?
        .ok_or_else(|| ApiError::not_found("找不到網段"))
}

/// 驗證路徑中的位址格式（v4／v6 皆可）。
///
/// 歷史是文字鍵：不做網段歸屬檢查，非本網段的位址回空歷史（由網段存在性
/// 負責 404）。
fn validate_address(text: &str) -> Result<String, ApiError> {
    text.parse::<IpAddr>()
        .map(|_| text.to_string())
        .map_err(|_| ApiError::validation(format!("位址格式錯誤：{text}")).field("address"))
}
