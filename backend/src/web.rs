//! 前端靜態檔服務（見 `docs/adr/0004`）。
//!
//! 建置產物存在時以 `ServeDir` 提供；找不到的檔案以 SPA index.html 回應
//! （history 路由 fallback，維持 200）。產物不存在時（例如尚未
//! `pnpm build:frontend`）回傳提示頁。

use std::path::Path;

use axum::Router;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use tower_http::services::{ServeDir, ServeFile};

/// 依 dist 目錄狀態回傳對應的 fallback 服務。
pub fn service(dist_dir: &Path) -> Router {
    let index = dist_dir.join("index.html");

    if index.exists() {
        Router::new().fallback_service(ServeDir::new(dist_dir).fallback(ServeFile::new(index)))
    } else {
        Router::new().fallback(get(not_built))
    }
}

const NOT_BUILT_HTML: &str = r#"<!doctype html>
<html lang="zh-Hant">
  <head><meta charset="utf-8"><title>asset-nest</title></head>
  <body>
    <h1>asset-nest 後端運行中</h1>
    <p>尚未找到前端建置產物。</p>
    <p>開發：請使用 Quasar dev server <a href="http://localhost:9000">http://localhost:9000</a></p>
    <p>建置：<code>pnpm build</code></p>
  </body>
</html>"#;

async fn not_built() -> Response {
    (StatusCode::OK, Html(NOT_BUILT_HTML)).into_response()
}
