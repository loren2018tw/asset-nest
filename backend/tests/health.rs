//! 骨架驗收：`/api/health` 回應 200 與 JSON 狀態。

use asset_nest::{AppState, app};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use sqlx::sqlite::SqlitePoolOptions;
use tower::ServiceExt;

#[tokio::test]
async fn health_returns_ok() {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("建立記憶體資料庫");

    let state = AppState::new(pool, std::env::temp_dir().join("asset-nest-no-dist"));

    let response = app(state)
        .oneshot(
            Request::builder()
                .uri("/api/health")
                .body(Body::empty())
                .expect("建立請求"),
        )
        .await
        .expect("執行請求");

    assert_eq!(response.status(), StatusCode::OK);

    let body = response
        .into_body()
        .collect()
        .await
        .expect("讀取回應內容")
        .to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).expect("回應為 JSON");

    assert_eq!(json["status"], "ok");
    assert_eq!(json["service"], "asset-nest");
    assert_eq!(json["database"], "ok");
}
