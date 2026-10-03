//! 認證邊界：目前為 no-op 插槽，未實作任何驗證。
//!
//! 功能階段將比照 Kealight ADR-0005 的簡易密碼機制實作。

use axum::extract::Request;
use axum::middleware::Next;
use axum::response::Response;

/// 認證 middleware 插槽：現階段直接放行。
pub async fn noop_auth(request: Request, next: Next) -> Response {
    next.run(request).await
}
