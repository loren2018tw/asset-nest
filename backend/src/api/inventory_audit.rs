//! `/api/v1` 清冊稽核路由（見 spec §5、票 01）。
//!
//! `POST /api/v1/inventory-audit`：multipart、檔案欄位 `file` 可重複 1～10 次；
//! 純唯讀（不寫入任何資料表、不落地檔案）。解析與比對一律走
//! [`crate::inventory_audit`]（單一權威）。

use axum::extract::DefaultBodyLimit;
use axum::extract::multipart::MultipartRejection;
use axum::extract::{Multipart, State};
use axum::routing::post;
use axum::{Json, Router};

use crate::AppState;
use crate::api::ApiError;
use crate::inventory_audit::{
    self, AuditReport, MAX_FILE_BYTES, MAX_FILES, UploadedFile, too_many_files_error,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/inventory-audit", post(audit_inventory))
        // 每檔 5 MB 上限由本端點自行檢查並整批回 400 檔級錯誤；
        // 放行 axum 預設 2 MB body limit（比照匯入端點）。
        .layer(DefaultBodyLimit::disable())
}

async fn audit_inventory(
    State(state): State<AppState>,
    multipart: Result<Multipart, MultipartRejection>,
) -> Result<Json<AuditReport>, ApiError> {
    let mut multipart = multipart.map_err(|_| multipart_error())?;
    let mut files: Vec<UploadedFile> = Vec::new();

    while let Some(mut field) = multipart
        .next_field()
        .await
        .map_err(|_| multipart_error())?
    {
        if field.name() != Some("file") {
            continue;
        }
        let filename = field.file_name().unwrap_or_default().to_string();

        // 第 11 檔即整批拒絕，不再讀取其內容（見 spec §4）。
        if files.len() >= MAX_FILES {
            let mut filenames: Vec<String> =
                files.iter().map(|file| file.filename.clone()).collect();
            filenames.push(filename);
            return Err(too_many_files_error(filenames));
        }

        // 單檔超過 5 MB 時不再保留內容，但持續耗盡欄位以讀取後續檔案，
        // 讓整批錯誤能逐檔回報（見 spec §4）。
        let mut bytes = Vec::new();
        let mut oversized = false;
        while let Some(chunk) = field.chunk().await.map_err(|_| multipart_error())? {
            if bytes.len() + chunk.len() > MAX_FILE_BYTES {
                oversized = true;
                continue;
            }
            bytes.extend_from_slice(&chunk);
        }
        files.push(UploadedFile {
            filename,
            bytes,
            oversized,
        });
    }

    if files.is_empty() {
        return Err(ApiError::validation("缺少檔案欄位 file").field("file"));
    }

    let report = inventory_audit::audit(&state.db, files).await?;
    Ok(Json(report))
}

fn multipart_error() -> ApiError {
    ApiError::validation("請求須為 multipart/form-data（檔案欄位 file）").field("file")
}
