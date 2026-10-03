//! API 錯誤格式：沿用 `{error, message}`，選配 `details`（見 spec §5）。

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use serde_json::{Value, json};

/// 統一的 API 錯誤；handler 以 `Result<_, ApiError>` 回傳。
#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    error: &'static str,
    message: String,
    details: Option<Value>,
}

impl ApiError {
    pub fn new(status: StatusCode, error: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            error,
            message: message.into(),
            details: None,
        }
    }

    /// 400：結構驗證錯誤（阻擋儲存，見 ADR-0006）。
    pub fn validation(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, "validation_error", message)
    }

    /// 標示錯誤對應的輸入欄位，供前端定位。
    pub fn field(mut self, field: &str) -> Self {
        self.details = Some(json!({ "field": field }));
        self
    }

    /// 404：資源不存在。
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, "not_found", message)
    }

    /// 500：記錄原始錯誤（含堆疊追蹤），對外僅回覆籠統訊息。
    pub fn internal(context: &'static str, source: impl std::fmt::Display) -> Self {
        tracing::error!(error = %source, "{context}");
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, "internal_error", context)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = ErrorBody {
            error: self.error,
            message: self.message,
            details: self.details,
        };
        (self.status, Json(body)).into_response()
    }
}

#[derive(Serialize)]
struct ErrorBody {
    error: &'static str,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    details: Option<Value>,
}
