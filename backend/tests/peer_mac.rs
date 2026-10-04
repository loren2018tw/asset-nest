//! 票 09 驗收：`GET /api/v1/peer-mac` 回應形狀。
//!
//! 測試以注入 `ConnectInfo` 模擬連線來源；TEST-NET 位址（203.0.113.0/24）
//! 不會出現在本機 ARP 表，故預期 `{ "mac": null }`。

use std::net::SocketAddr;

use asset_nest::{AppState, app};
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use sqlx::sqlite::SqlitePoolOptions;
use tower::ServiceExt;

async fn get_peer_mac(remote: SocketAddr) -> (StatusCode, serde_json::Value) {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("建立記憶體資料庫");
    let state = AppState::new(pool, std::env::temp_dir().join("asset-nest-no-dist"));

    let mut request = Request::builder()
        .uri("/api/v1/peer-mac")
        .body(Body::empty())
        .expect("建立請求");
    request.extensions_mut().insert(ConnectInfo(remote));

    let response = app(state).oneshot(request).await.expect("執行請求");
    let status = response.status();

    let body = response
        .into_body()
        .collect()
        .await
        .expect("讀取回應內容")
        .to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).expect("回應為 JSON");

    (status, json)
}

#[tokio::test]
async fn peer_mac_v4_without_arp_entry_returns_null() {
    let remote: SocketAddr = "203.0.113.7:54321".parse().expect("測試來源位址");
    let (status, json) = get_peer_mac(remote).await;

    assert_eq!(status, StatusCode::OK);
    assert!(json.get("mac").is_some(), "回應應含 mac 欄位：{json}");
    assert!(json["mac"].is_null(), "ARP 表查不到的來源應回 null：{json}");
}

#[tokio::test]
async fn peer_mac_v6_returns_null() {
    let remote: SocketAddr = "[2001:db8::7]:54321".parse().expect("測試來源位址");
    let (status, json) = get_peer_mac(remote).await;

    assert_eq!(status, StatusCode::OK);
    assert!(json["mac"].is_null(), "IPv6 不做反查，應回 null：{json}");
}
