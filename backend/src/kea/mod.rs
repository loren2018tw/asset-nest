//! Kea 整合邊界：整合機制為伺服器內建 HTTP 控制通道（見 `docs/adr/0010`）。
//!
//! 控制通道 client 見 [`http`]；即時單筆推送見 [`push`]（見 `docs/adr/0027`）、
//! 完整同步見 [`sync`]（決策見 `docs/adr/0011`）。

pub mod http;
pub mod push;
pub mod sync;

/// Kea 整合錯誤。
#[derive(Debug, thiserror::Error)]
pub enum KeaError {
    #[error("HTTP 請求失敗：{0}")]
    Http(#[from] reqwest::Error),
    #[error("Kea 回應錯誤（result={result}）：{text}")]
    Response { result: i64, text: String },
    #[error("Kea 回應格式不符：{0}")]
    Malformed(String),
    #[error("Kea 推送資料異常：{0}")]
    Data(String),
}
