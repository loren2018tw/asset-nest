//! 路由骨架驗收：SPA history fallback 與 API 404 行為。

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use asset_nest::{AppState, app};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use sqlx::SqlitePool;
use sqlx::sqlite::SqlitePoolOptions;
use tower::ServiceExt;

const INDEX_HTML: &str = "<!doctype html><title>spa-index</title>";

async fn memory_pool() -> SqlitePool {
    SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("建立記憶體資料庫")
}

fn unique_temp_dir() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("系統時間")
        .as_nanos();
    std::env::temp_dir().join(format!("asset-nest-test-{nanos}"))
}

async fn get(uri: &str, dist_dir: PathBuf) -> axum::response::Response {
    app(AppState::new(memory_pool().await, dist_dir))
        .oneshot(
            Request::builder()
                .uri(uri)
                .body(Body::empty())
                .expect("建立請求"),
        )
        .await
        .expect("執行請求")
}

#[tokio::test]
async fn spa_fallback_serves_index_with_200() {
    let dir = unique_temp_dir();
    fs::create_dir_all(&dir).expect("建立測試 dist 目錄");
    fs::write(dir.join("index.html"), INDEX_HTML).expect("寫入 index.html");

    let response = get("/client/side/route", dir).await;

    assert_eq!(response.status(), StatusCode::OK);

    let body = response
        .into_body()
        .collect()
        .await
        .expect("讀取回應內容")
        .to_bytes();

    assert!(
        String::from_utf8_lossy(&body).contains("spa-index"),
        "fallback 應回傳 index.html 內容"
    );
}

#[tokio::test]
async fn unknown_api_returns_json_404() {
    let response = get("/api/unknown", unique_temp_dir()).await;

    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_string();

    assert!(
        content_type.starts_with("application/json"),
        "API 404 應為 JSON，實際為 {content_type}"
    );
}
