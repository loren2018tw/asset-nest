//! `/api/v1` 資產 CSV 匯入路由（見 spec §5、票 02）。
//!
//! `POST /api/v1/assets/import?dry_run=true|false`：multipart、檔案欄位 `file`；
//! `dry_run` 預設 `true`。解析與驗證一律走 [`crate::import`]（單一驗證權威）。
//! 靜態路徑 `/assets/import` 與既有 `/assets/{id}` 動態路由並存，axum 以靜態優先。

use axum::extract::multipart::MultipartRejection;
use axum::extract::rejection::QueryRejection;
use axum::extract::{DefaultBodyLimit, Multipart, Query, State};
use axum::routing::post;
use axum::{Json, Router};
use serde::Deserialize;

use crate::AppState;
use crate::api::ApiError;
use crate::import::{self, ImportReport, MAX_FILE_BYTES};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/assets/import", post(import_assets))
        // 檔案上限（5 MB）由匯入端點自行檢查並回 400 報告；放行 axum 預設 2 MB
        // body limit，避免 5 MB 內的合法檔案先被擋掉（見 spec §2、票 02）。
        .layer(DefaultBodyLimit::disable())
}

#[derive(Debug, Deserialize)]
struct ImportQuery {
    /// 預覽（`true`，預設）或正式匯入（`false`）。
    dry_run: Option<bool>,
}

async fn import_assets(
    State(state): State<AppState>,
    query: Result<Query<ImportQuery>, QueryRejection>,
    multipart: Result<Multipart, MultipartRejection>,
) -> Result<Json<ImportReport>, ApiError> {
    let Query(query) =
        query.map_err(|_| ApiError::validation("查詢參數格式錯誤").field("dry_run"))?;
    let dry_run = query.dry_run.unwrap_or(true);

    let mut multipart = multipart.map_err(|_| multipart_error())?;
    let mut file: Option<Vec<u8>> = None;
    while let Some(mut field) = multipart
        .next_field()
        .await
        .map_err(|_| multipart_error())?
    {
        if field.name() != Some("file") {
            continue;
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = field.chunk().await.map_err(|_| multipart_error())? {
            if bytes.len() + chunk.len() > MAX_FILE_BYTES {
                return Err(ApiError::validation("檔案超過 5 MB 上限").field("file"));
            }
            bytes.extend_from_slice(&chunk);
        }
        file = Some(bytes);
        break;
    }
    let bytes = file.ok_or_else(|| ApiError::validation("缺少檔案欄位 file").field("file"))?;

    let mut analysis = import::analyze(&state.db, &bytes).await?;
    analysis.report.dry_run = dry_run;

    // 正式匯入：先以當下資料重驗（此即 analyze 的結果）；有結構錯誤整批不寫入。
    if !dry_run && !analysis.has_errors() {
        let created = import::commit(&state.db, analysis.plans).await?;
        analysis.report.committed = true;
        analysis.report.created = Some(created);
    }

    Ok(Json(analysis.report))
}

fn multipart_error() -> ApiError {
    ApiError::validation("請求須為 multipart/form-data（檔案欄位 file）").field("file")
}
