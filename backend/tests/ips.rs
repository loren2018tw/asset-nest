//! v4 IP 清單整合測試：枚舉邊界、pool/gateway 標示、搜尋與分頁（見票 04）。

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use sqlx::SqlitePool;
use sqlx::sqlite::SqlitePoolOptions;
use tower::ServiceExt;

use asset_nest::{AppState, app};

/// 建立測試資料庫並套用 migrations（領域測試必須先套）。
async fn test_pool() -> SqlitePool {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("建立記憶體資料庫");

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("套用 migrations");

    pool
}

/// 以 `oneshot` 發送請求；回傳狀態碼與 JSON（204 等空內容為 `Value::Null`）。
async fn send(
    pool: &SqlitePool,
    method: Method,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(uri);
    let body = match body {
        Some(value) => {
            builder = builder.header(header::CONTENT_TYPE, "application/json");
            Body::from(value.to_string())
        }
        None => Body::empty(),
    };

    let response = app(AppState::new(
        pool.clone(),
        std::env::temp_dir().join("asset-nest-test-no-dist"),
    ))
    .oneshot(builder.body(body).expect("建立請求"))
    .await
    .expect("執行請求");

    let status = response.status();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("讀取回應內容")
        .to_bytes();

    let json = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).expect("回應為 JSON")
    };

    (status, json)
}

/// 新增網段並斷言成功，回傳回應 JSON。
async fn create_subnet(pool: &SqlitePool, body: Value) -> Value {
    let (status, json) = send(pool, Method::POST, "/api/v1/subnets", Some(body)).await;
    assert_eq!(status, StatusCode::CREATED, "新增網段應成功：{json}");
    json
}

/// 讀取某網段的 IP 清單（`query` 含開頭 `?` 或空字串）並斷言成功。
async fn list_ips(pool: &SqlitePool, id: i64, query: &str) -> Value {
    let (status, json) = send(
        pool,
        Method::GET,
        &format!("/api/v1/subnets/{id}/ips{query}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "讀取 IP 清單應成功：{json}");
    json
}

/// 取出回應列中的位址字串。
fn addresses(page: &Value) -> Vec<&str> {
    page["items"]
        .as_array()
        .expect("items 為陣列")
        .iter()
        .map(|item| item["address"].as_str().expect("address 為字串"))
        .collect()
}

#[tokio::test]
async fn v4_subnet_lists_all_hosts_in_numeric_order() {
    let pool = test_pool().await;
    let subnet = create_subnet(&pool, json!({ "cidr": "10.0.0.0/29" })).await;
    let id = subnet["id"].as_i64().expect("回應含 id");

    // 預設：每頁 50 筆、扣除 network/broadcast、數值升冪
    let page = list_ips(&pool, id, "").await;
    assert_eq!(page["total"], 6);
    assert_eq!(page["page"], 1);
    assert_eq!(page["per_page"], 50, "預設每頁 50 筆");
    assert_eq!(
        addresses(&page),
        [
            "10.0.0.1", "10.0.0.2", "10.0.0.3", "10.0.0.4", "10.0.0.5", "10.0.0.6"
        ]
    );

    // 本票尚無指派：列含預留欄位（status/purpose/conflicts）
    let first = &page["items"][0];
    assert_eq!(first["in_pool"], false);
    assert_eq!(first["is_gateway"], false);
    assert_eq!(first["status"], "available");
    assert!(first["purpose"].is_null());
    assert_eq!(first["conflicts"], json!([]));

    // 伺服器端分頁
    let page = list_ips(&pool, id, "?page=2&per_page=2").await;
    assert_eq!(page["total"], 6);
    assert_eq!(page["page"], 2);
    assert_eq!(page["per_page"], 2);
    assert_eq!(addresses(&page), ["10.0.0.3", "10.0.0.4"]);

    // 超出範圍的頁：空列但總數不變
    let page = list_ips(&pool, id, "?page=10&per_page=50").await;
    assert_eq!(page["total"], 6);
    assert!(addresses(&page).is_empty());

    // page 下限 1、per_page 上限 200、下限 1（比照 /assets）
    let page = list_ips(&pool, id, "?page=0").await;
    assert_eq!(page["page"], 1);
    let page = list_ips(&pool, id, "?per_page=1000").await;
    assert_eq!(page["per_page"], 200);
    assert_eq!(addresses(&page).len(), 6);
    let page = list_ips(&pool, id, "?per_page=0").await;
    assert_eq!(page["per_page"], 1);
    assert_eq!(addresses(&page).len(), 1);
}

#[tokio::test]
async fn slash31_and_slash32_list_every_address() {
    let pool = test_pool().await;

    // /31：全數列出（含 network/broadcast）
    let subnet = create_subnet(&pool, json!({ "cidr": "10.0.1.0/31" })).await;
    let id = subnet["id"].as_i64().expect("回應含 id");
    let page = list_ips(&pool, id, "").await;
    assert_eq!(page["total"], 2);
    assert_eq!(addresses(&page), ["10.0.1.0", "10.0.1.1"]);

    // /32：單一位址
    let subnet = create_subnet(&pool, json!({ "cidr": "10.0.2.7/32" })).await;
    let id = subnet["id"].as_i64().expect("回應含 id");
    let page = list_ips(&pool, id, "").await;
    assert_eq!(page["total"], 1);
    assert_eq!(addresses(&page), ["10.0.2.7"]);
}

#[tokio::test]
async fn pool_and_gateway_flags_are_reported() {
    let pool = test_pool().await;

    // gateway 不在 pool 內；pool 有兩段（含單一位址段）
    let subnet = create_subnet(
        &pool,
        json!({
            "cidr": "10.0.0.0/29",
            "gateway": "10.0.0.1",
            "pools": [
                { "start_ip": "10.0.0.2", "end_ip": "10.0.0.3" },
                { "start_ip": "10.0.0.6", "end_ip": "10.0.0.6" }
            ]
        }),
    )
    .await;
    let id = subnet["id"].as_i64().expect("回應含 id");

    let page = list_ips(&pool, id, "").await;
    assert_eq!(page["total"], 6);
    let items = page["items"].as_array().expect("items 為陣列");
    let item = |address: &str| {
        items
            .iter()
            .find(|item| item["address"] == address)
            .unwrap_or_else(|| panic!("列含 {address}"))
    };

    // gateway：標記但非池內、仍為可用（見 spec §7）
    assert_eq!(item("10.0.0.1")["is_gateway"], true);
    assert_eq!(item("10.0.0.1")["in_pool"], false);
    assert_eq!(item("10.0.0.1")["status"], "available");

    // pool 內：標示「池內」且無指派用途
    for address in ["10.0.0.2", "10.0.0.3", "10.0.0.6"] {
        assert_eq!(item(address)["in_pool"], true, "{address} 在 pool 內");
        assert_eq!(item(address)["status"], "in_pool", "{address} 狀態為池內");
        assert_eq!(item(address)["is_gateway"], false);
        assert!(item(address)["purpose"].is_null());
    }

    // pool 外：可用
    assert_eq!(item("10.0.0.4")["in_pool"], false);
    assert_eq!(item("10.0.0.4")["status"], "available");

    // gateway 落在 pool 內時兩個標記並存（pool 涵蓋 gateway 提示不擋，見 spec §6）
    let subnet = create_subnet(
        &pool,
        json!({
            "cidr": "10.0.3.0/30",
            "gateway": "10.0.3.1",
            "pools": [{ "start_ip": "10.0.3.1", "end_ip": "10.0.3.2" }]
        }),
    )
    .await;
    let id = subnet["id"].as_i64().expect("回應含 id");
    let page = list_ips(&pool, id, "").await;
    let gateway = &page["items"][0];
    assert_eq!(gateway["address"], "10.0.3.1");
    assert_eq!(gateway["is_gateway"], true);
    assert_eq!(gateway["in_pool"], true);
    assert_eq!(gateway["status"], "in_pool");
}

#[tokio::test]
async fn search_matches_exact_address_or_substring() {
    let pool = test_pool().await;
    let subnet = create_subnet(&pool, json!({ "cidr": "10.0.0.0/28" })).await;
    let id = subnet["id"].as_i64().expect("回應含 id");

    // 完整位址：精確比對，僅一筆
    let page = list_ips(&pool, id, "?q=10.0.0.10").await;
    assert_eq!(page["total"], 1);
    assert_eq!(addresses(&page), ["10.0.0.10"]);

    // 部分關鍵字：位址文字子字串比對（.1x 共六筆）
    let page = list_ips(&pool, id, "?q=.1").await;
    assert_eq!(page["total"], 6);
    assert_eq!(
        addresses(&page),
        [
            "10.0.0.1",
            "10.0.0.10",
            "10.0.0.11",
            "10.0.0.12",
            "10.0.0.13",
            "10.0.0.14"
        ]
    );

    // 子字串搜尋同樣伺服器端分頁
    let page = list_ips(&pool, id, "?q=.1&page=2&per_page=2").await;
    assert_eq!(page["total"], 6);
    assert_eq!(addresses(&page), ["10.0.0.11", "10.0.0.12"]);

    // 完整位址不在範圍內：空結果
    let page = list_ips(&pool, id, "?q=10.0.1.1").await;
    assert_eq!(page["total"], 0);
    assert!(addresses(&page).is_empty());

    // 無符合的子字串：空結果
    let page = list_ips(&pool, id, "?q=255").await;
    assert_eq!(page["total"], 0);
}

#[tokio::test]
async fn v6_subnet_lists_no_free_addresses() {
    let pool = test_pool().await;
    let subnet = create_subnet(&pool, json!({ "cidr": "fd00::/64" })).await;
    let id = subnet["id"].as_i64().expect("回應含 id");

    // v6 為登錄制（票 06）：尚無登錄時清單為空，不枚舉空閒位址
    let page = list_ips(&pool, id, "").await;
    assert_eq!(page["total"], 0);
    assert_eq!(page["page"], 1);
    assert_eq!(page["per_page"], 50);
    assert!(addresses(&page).is_empty());

    // 未知狀態值仍回 400
    let (status, body) = send(
        &pool,
        Method::GET,
        &format!("/api/v1/subnets/{id}/ips?status=dhcp"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "validation_error");
    assert_eq!(body["details"]["field"], "status");
}

#[tokio::test]
async fn unknown_subnet_returns_404() {
    let pool = test_pool().await;

    let (status, body) = send(&pool, Method::GET, "/api/v1/subnets/999/ips", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"], "not_found");

    let (status, body) = send(&pool, Method::GET, "/api/v1/subnets/abc/ips", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "validation_error");
}
