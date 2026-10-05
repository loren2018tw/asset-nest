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

    /// 附加結構錯誤的額外細節（如衝突網段）；可與 [`ApiError::field`] 併用。
    pub fn detail(mut self, key: &str, value: Value) -> Self {
        let mut map = match self.details.take() {
            Some(Value::Object(map)) => map,
            _ => serde_json::Map::new(),
        };
        map.insert(key.to_string(), value);
        self.details = Some(Value::Object(map));
        self
    }

    /// 404：資源不存在。
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, "not_found", message)
    }

    /// 409：與資源目前狀態衝突（如刪除非空網段）；請求本身合法（見票 08）。
    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, "conflict", message)
    }

    /// 501：端點已存在，但此情境尚未實作。
    pub fn not_implemented(message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_IMPLEMENTED, "not_implemented", message)
    }

    /// 502：Kea 控制通道連線或命令失敗（如完整同步計畫無法產生）。
    pub fn kea(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_GATEWAY, "kea_error", message)
    }

    /// 500：記錄原始錯誤（含堆疊追蹤），對外僅回覆籠統訊息。
    pub fn internal(context: &'static str, source: impl std::fmt::Display) -> Self {
        tracing::error!(error = %source, "{context}");
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, "internal_error", context)
    }

    /// 讀取訊息內容（供匯入逐列報告重用領域驗證錯誤；見票 02）。
    pub(crate) fn message(&self) -> &str {
        &self.message
    }

    /// 讀取 `details.field`；未標示欄位時回傳 `None`（供匯入對應 CSV 欄位）。
    pub(crate) fn field_name(&self) -> Option<&str> {
        self.details.as_ref()?.get("field")?.as_str()
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
