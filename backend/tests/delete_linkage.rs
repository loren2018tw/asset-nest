//! 刪除連動與防護整合測試：資產／介面連動刪除、非空網段防護（見票 08）。

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

/// 新增資產並斷言成功，回傳 id。
async fn create_asset(pool: &SqlitePool, description: &str, location: &str) -> i64 {
    let (status, json) = send(
        pool,
        Method::POST,
        "/api/v1/assets",
        Some(json!({ "description": description, "location": location })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "新增資產應成功：{json}");
    json["id"].as_i64().expect("回應含 id")
}

/// 對資產新增介面並斷言成功，回傳 id。
async fn create_interface(pool: &SqlitePool, asset_id: i64, body: Value) -> i64 {
    let (status, json) = send(
        pool,
        Method::POST,
        &format!("/api/v1/assets/{asset_id}/interfaces"),
        Some(body),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "新增介面應成功：{json}");
    json["id"].as_i64().expect("回應含 id")
}

/// 新增網段並斷言成功，回傳 id。
async fn create_subnet(pool: &SqlitePool, body: Value) -> i64 {
    let (status, json) = send(pool, Method::POST, "/api/v1/subnets", Some(body)).await;
    assert_eq!(status, StatusCode::CREATED, "新增網段應成功：{json}");
    json["id"].as_i64().expect("回應含 id")
}

/// 指派／改用途（不檢查狀態碼，供各測試自行斷言）。
async fn put_assignment(
    pool: &SqlitePool,
    subnet_id: i64,
    address: &str,
    body: Value,
) -> (StatusCode, Value) {
    send(
        pool,
        Method::PUT,
        &format!("/api/v1/subnets/{subnet_id}/ips/{address}/assignment"),
        Some(body),
    )
    .await
}

/// 取消指派（不檢查狀態碼）。
async fn delete_assignment(
    pool: &SqlitePool,
    subnet_id: i64,
    address: &str,
) -> (StatusCode, Value) {
    send(
        pool,
        Method::DELETE,
        &format!("/api/v1/subnets/{subnet_id}/ips/{address}/assignment"),
        None,
    )
    .await
}

/// v6 登錄位址（新增即指派）。
async fn register_ip(
    pool: &SqlitePool,
    subnet_id: i64,
    address: &str,
    interface_id: i64,
) -> (StatusCode, Value) {
    send(
        pool,
        Method::POST,
        &format!("/api/v1/subnets/{subnet_id}/ips"),
        Some(json!({ "address": address, "interface_id": interface_id })),
    )
    .await
}

/// 讀取某網段的 IP 清單並斷言成功。
async fn list_ips(pool: &SqlitePool, subnet_id: i64, query: &str) -> Value {
    let (status, json) = send(
        pool,
        Method::GET,
        &format!("/api/v1/subnets/{subnet_id}/ips{query}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "讀取 IP 清單應成功：{json}");
    json
}

/// 取出 IP 清單中某位址的列。
fn row<'a>(page: &'a Value, address: &str) -> &'a Value {
    page["items"]
        .as_array()
        .expect("items 為陣列")
        .iter()
        .find(|item| item["address"] == address)
        .unwrap_or_else(|| panic!("清單含 {address}"))
}

/// 直接查資料庫：某資產的介面 id 清單。
async fn interface_ids(pool: &SqlitePool, asset_id: i64) -> Vec<i64> {
    sqlx::query_scalar("SELECT id FROM interfaces WHERE asset_id = ? ORDER BY id")
        .bind(asset_id)
        .fetch_all(pool)
        .await
        .expect("查詢介面")
}

/// 直接查資料庫：某資產（經由介面）的指派筆數。
async fn asset_assignment_count(pool: &SqlitePool, asset_id: i64) -> i64 {
    sqlx::query_scalar(
        "SELECT COUNT(*)
           FROM ip_assignments a
           JOIN interfaces i ON i.id = a.interface_id
          WHERE i.asset_id = ?",
    )
    .bind(asset_id)
    .fetch_one(pool)
    .await
    .expect("查詢指派")
}

/// 直接查資料庫：某網段的指派筆數。
async fn subnet_assignment_count(pool: &SqlitePool, subnet_id: i64) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM ip_assignments WHERE subnet_id = ?")
        .bind(subnet_id)
        .fetch_one(pool)
        .await
        .expect("查詢指派")
}

#[tokio::test]
async fn deleting_asset_cascades_interfaces_assignments_and_reservations() {
    let pool = test_pool().await;

    // 目標資產：兩個介面、v4 手動＋保留、v6 登錄
    let asset_id = create_asset(&pool, "目標主機", "機房 A").await;
    let first = create_interface(
        &pool,
        asset_id,
        json!({ "name": "eth0", "mac": "aa:bb:cc:dd:ee:01" }),
    )
    .await;
    let second = create_interface(
        &pool,
        asset_id,
        json!({ "name": "eth1", "mac": "aa:bb:cc:dd:ee:02" }),
    )
    .await;
    let v4 = create_subnet(&pool, json!({ "cidr": "10.0.0.0/29" })).await;
    let v6 = create_subnet(&pool, json!({ "cidr": "fd00::/64" })).await;

    let (status, _) = put_assignment(
        &pool,
        v4,
        "10.0.0.1",
        json!({ "interface_id": first, "purpose": "static" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = put_assignment(
        &pool,
        v4,
        "10.0.0.2",
        json!({
            "interface_id": second,
            "purpose": "reservation",
            "hostname": "target-1"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = register_ip(&pool, v6, "fd00::1", first).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(asset_assignment_count(&pool, asset_id).await, 3);

    // 對照組：另一資產的介面與指派不受影響
    let other_asset = create_asset(&pool, "保留主機", "機房 B").await;
    let other_interface = create_interface(&pool, other_asset, json!({ "name": "eth0" })).await;
    let (status, _) = put_assignment(
        &pool,
        v4,
        "10.0.0.3",
        json!({ "interface_id": other_interface, "purpose": "static" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // 刪除資產 → 介面與指派（含保留、v6 登錄）全部連動刪除
    let (status, body) = send(
        &pool,
        Method::DELETE,
        &format!("/api/v1/assets/{asset_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(body, Value::Null);

    assert_eq!(
        interface_ids(&pool, asset_id).await,
        Vec::<i64>::new(),
        "介面全數連動刪除"
    );
    assert_eq!(
        asset_assignment_count(&pool, asset_id).await,
        0,
        "指派（含保留與 v6）全數連動刪除"
    );

    // 對照組不受影響
    assert_eq!(
        interface_ids(&pool, other_asset).await,
        vec![other_interface]
    );
    assert_eq!(asset_assignment_count(&pool, other_asset).await, 1);

    // 位址回到可用；v6 登錄消失
    let page = list_ips(&pool, v4, "").await;
    assert_eq!(row(&page, "10.0.0.1")["status"], "available");
    assert_eq!(row(&page, "10.0.0.2")["status"], "available");
    assert_eq!(row(&page, "10.0.0.3")["status"], "static");
    assert_eq!(list_ips(&pool, v6, "").await["total"], 0, "v6 登錄連動移除");

    // 詳情回 404
    let (status, _) = send(
        &pool,
        Method::GET,
        &format!("/api/v1/assets/{asset_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn deleting_interface_cascades_its_assignments() {
    let pool = test_pool().await;
    let asset_id = create_asset(&pool, "雙網卡主機", "機房 A").await;
    let removed = create_interface(
        &pool,
        asset_id,
        json!({ "name": "eth0", "mac": "aa:bb:cc:dd:ee:01" }),
    )
    .await;
    let kept = create_interface(
        &pool,
        asset_id,
        json!({ "name": "eth1", "mac": "aa:bb:cc:dd:ee:02" }),
    )
    .await;
    let v4 = create_subnet(&pool, json!({ "cidr": "10.0.0.0/29" })).await;
    let v6 = create_subnet(&pool, json!({ "cidr": "fd00::/64" })).await;

    // 被刪介面在 v4 手動、v6 登錄各一；另一介面有保留
    let (status, _) = put_assignment(
        &pool,
        v4,
        "10.0.0.1",
        json!({ "interface_id": removed, "purpose": "static" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = register_ip(&pool, v6, "fd00::1", removed).await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, _) = put_assignment(
        &pool,
        v4,
        "10.0.0.2",
        json!({ "interface_id": kept, "purpose": "reservation" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(asset_assignment_count(&pool, asset_id).await, 3);

    // 刪除介面 → 其指派（跨網段）連動刪除
    let (status, body) = send(
        &pool,
        Method::DELETE,
        &format!("/api/v1/interfaces/{removed}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(body, Value::Null);

    let remaining: Vec<(i64, String)> =
        sqlx::query_as("SELECT interface_id, address FROM ip_assignments ORDER BY id")
            .fetch_all(&pool)
            .await
            .expect("查詢剩餘指派");
    assert_eq!(
        remaining,
        vec![(kept, "10.0.0.2".to_string())],
        "僅保留另一介面的指派"
    );
    assert_eq!(asset_assignment_count(&pool, asset_id).await, 1);

    let page = list_ips(&pool, v4, "").await;
    assert_eq!(row(&page, "10.0.0.1")["status"], "available");
    assert_eq!(row(&page, "10.0.0.2")["status"], "reservation");
    assert_eq!(list_ips(&pool, v6, "").await["total"], 0);
}

#[tokio::test]
async fn subnet_with_assignments_cannot_be_deleted() {
    let pool = test_pool().await;
    let asset_id = create_asset(&pool, "測試主機", "機房 A").await;
    let interface_id = create_interface(
        &pool,
        asset_id,
        json!({ "name": "eth0", "mac": "aa:bb:cc:dd:ee:ff" }),
    )
    .await;

    // v4：手動＋保留共 2 筆
    let v4 = create_subnet(&pool, json!({ "cidr": "10.0.0.0/29", "name": "辦公區" })).await;
    let second_interface = create_interface(
        &pool,
        asset_id,
        json!({ "name": "eth1", "mac": "aa:bb:cc:dd:ee:fe" }),
    )
    .await;
    let (status, _) = put_assignment(
        &pool,
        v4,
        "10.0.0.1",
        json!({ "interface_id": interface_id, "purpose": "static" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = put_assignment(
        &pool,
        v4,
        "10.0.0.2",
        json!({ "interface_id": second_interface, "purpose": "reservation" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // v6：登錄 1 筆
    let v6 = create_subnet(&pool, json!({ "cidr": "fd00::/64" })).await;
    let (status, _) = register_ip(&pool, v6, "fd00::1", interface_id).await;
    assert_eq!(status, StatusCode::CREATED);

    // v4 刪除被擋：409、結構化錯誤附數量，訊息明確
    let (status, body) = send(
        &pool,
        Method::DELETE,
        &format!("/api/v1/subnets/{v4}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"], "conflict");
    assert_eq!(body["details"]["assignments"], 2, "含保留在內共 2 筆");
    let message = body["message"].as_str().expect("訊息為字串");
    assert!(message.contains("辦公區"), "訊息指出網段：{message}");
    assert!(message.contains('2'), "訊息含指派數量：{message}");
    assert!(message.contains("不可刪除"), "訊息說明不可刪除：{message}");

    // 網段與指派仍在，未誤刪
    let (status, _) = send(&pool, Method::GET, &format!("/api/v1/subnets/{v4}"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(subnet_assignment_count(&pool, v4).await, 2);

    // v6 刪除同樣被擋（登錄即指派）
    let (status, body) = send(
        &pool,
        Method::DELETE,
        &format!("/api/v1/subnets/{v6}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"], "conflict");
    assert_eq!(body["details"]["assignments"], 1);
    let message = body["message"].as_str().expect("訊息為字串");
    assert!(message.contains('1'), "訊息含指派數量：{message}");

    // 取消 v4 所有指派 → 可刪（取消指派回應 200；未涉及 Kea）
    let (status, _) = delete_assignment(&pool, v4, "10.0.0.1").await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = delete_assignment(&pool, v4, "10.0.0.2").await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = send(
        &pool,
        Method::DELETE,
        &format!("/api/v1/subnets/{v4}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // 取消 v6 登錄 → 可刪（取消指派回應 200）
    let (status, _) = delete_assignment(&pool, v6, "fd00::1").await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = send(
        &pool,
        Method::DELETE,
        &format!("/api/v1/subnets/{v6}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn empty_subnet_can_be_deleted_with_pools() {
    let pool = test_pool().await;

    // 有 pool 但無指派：可刪，pools 連動刪除
    let subnet_id = create_subnet(
        &pool,
        json!({
            "cidr": "10.9.0.0/29",
            "pools": [{ "start_ip": "10.9.0.1", "end_ip": "10.9.0.3" }]
        }),
    )
    .await;

    let (status, body) = send(
        &pool,
        Method::DELETE,
        &format!("/api/v1/subnets/{subnet_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(body, Value::Null);

    let remaining: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM subnet_pools")
        .fetch_one(&pool)
        .await
        .expect("查詢 pools");
    assert_eq!(remaining, 0, "刪除網段連動刪除 pools");

    // 重複刪除與不存在的網段：404（不是 409）
    for id in [subnet_id, 9999] {
        let (status, body) = send(
            &pool,
            Method::DELETE,
            &format!("/api/v1/subnets/{id}"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "id={id}");
        assert_eq!(body["error"], "not_found");
    }
}

#[tokio::test]
async fn empty_subnet_can_be_deleted_with_exclusions() {
    let pool = test_pool().await;

    // 有排除範圍但無指派：可刪，subnet_exclusions 連動刪除
    let subnet_id = create_subnet(
        &pool,
        json!({
            "cidr": "10.14.0.0/29",
            "exclusions": [
                { "start_ip": "10.14.0.1", "end_ip": "10.14.0.2", "note": "NAT 對外" }
            ]
        }),
    )
    .await;

    let (status, body) = send(
        &pool,
        Method::DELETE,
        &format!("/api/v1/subnets/{subnet_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(body, Value::Null);

    let remaining: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM subnet_exclusions")
        .fetch_one(&pool)
        .await
        .expect("查詢排除範圍");
    assert_eq!(remaining, 0, "刪除網段連動刪除 subnet_exclusions");
}
