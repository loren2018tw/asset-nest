//! IP 指派整合測試：指派／改用途、取消、結構規則與連動刪除（見票 05）。

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

/// 新增含財產編號的資產並斷言成功，回傳 id（供票 15 顯示格式斷言）。
async fn create_asset_with_property_no(
    pool: &SqlitePool,
    property_no: &str,
    description: &str,
    location: &str,
) -> i64 {
    let (status, json) = send(
        pool,
        Method::POST,
        "/api/v1/assets",
        Some(json!({
            "property_no": property_no,
            "description": description,
            "location": location
        })),
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

/// 讀取資產詳情並斷言成功。
async fn get_asset(pool: &SqlitePool, asset_id: i64) -> Value {
    let (status, json) = send(
        pool,
        Method::GET,
        &format!("/api/v1/assets/{asset_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "讀取資產詳情應成功：{json}");
    json
}

/// 指派列總數（直接查資料庫）。
async fn assignment_count(pool: &SqlitePool) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM ip_assignments")
        .fetch_one(pool)
        .await
        .expect("查詢指派筆數")
}

#[tokio::test]
async fn static_assignment_and_cancel_round_trip() {
    let pool = test_pool().await;
    let asset_id = create_asset_with_property_no(&pool, "P-001", "資料庫主機", "機房 A").await;
    let interface_id = create_interface(
        &pool,
        asset_id,
        json!({ "name": "eth0", "mac": "AA:BB:CC:DD:EE:FF" }),
    )
    .await;
    let subnet_id = create_subnet(&pool, json!({ "cidr": "10.0.0.0/29" })).await;

    // 指派為手動設定
    let (status, created) = put_assignment(
        &pool,
        subnet_id,
        "10.0.0.1",
        json!({ "interface_id": interface_id, "purpose": "static" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "指派應成功：{created}");
    assert_eq!(created["subnet_id"], subnet_id);
    assert_eq!(created["address"], "10.0.0.1");
    assert_eq!(created["interface_id"], interface_id);
    assert_eq!(created["purpose"], "static");
    assert!(created["hostname"].is_null());

    // 重複送出相同內容：冪等更新，不新增列
    let (status, _) = put_assignment(
        &pool,
        subnet_id,
        "10.0.0.1",
        json!({ "interface_id": interface_id, "purpose": "static" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(assignment_count(&pool).await, 1);

    // IP 清單顯示指派對象（資產描述＋位置、介面名稱／MAC）
    let page = list_ips(&pool, subnet_id, "").await;
    let assigned = row(&page, "10.0.0.1");
    assert_eq!(assigned["status"], "static");
    assert_eq!(assigned["purpose"], "static");
    assert_eq!(assigned["in_pool"], false);
    assert_eq!(assigned["assignment"]["asset_id"], asset_id);
    assert_eq!(assigned["assignment"]["asset_property_no"], "P-001");
    assert_eq!(assigned["assignment"]["asset_description"], "資料庫主機");
    assert_eq!(assigned["assignment"]["asset_location"], "機房 A");
    assert_eq!(assigned["assignment"]["interface_id"], interface_id);
    assert_eq!(assigned["assignment"]["interface_name"], "eth0");
    assert_eq!(assigned["assignment"]["mac"], "aa:bb:cc:dd:ee:ff");
    assert!(assigned["assignment"]["hostname"].is_null());
    assert_eq!(assigned["conflicts"], json!([]), "衝突欄位預留票 07");

    // 資產詳情含已指派 IP（唯讀顯示用；含網段資訊）
    let detail = get_asset(&pool, asset_id).await;
    let assignments = detail["assignments"]
        .as_array()
        .expect("assignments 為陣列");
    assert_eq!(assignments.len(), 1);
    assert_eq!(assignments[0]["address"], "10.0.0.1");
    assert_eq!(assignments[0]["subnet_id"], subnet_id);
    assert_eq!(assignments[0]["subnet_cidr"], "10.0.0.0/29");
    assert!(assignments[0]["subnet_name"].is_null());
    assert_eq!(assignments[0]["purpose"], "static");
    assert_eq!(assignments[0]["interface_id"], interface_id);
    assert_eq!(assignments[0]["interface_name"], "eth0");
    assert_eq!(assignments[0]["mac"], "aa:bb:cc:dd:ee:ff");

    // 改用途為保留（含 hostname）
    let (status, updated) = put_assignment(
        &pool,
        subnet_id,
        "10.0.0.1",
        json!({
            "interface_id": interface_id,
            "purpose": "reservation",
            "hostname": " db-1 "
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "改用途應成功：{updated}");
    assert_eq!(updated["purpose"], "reservation");
    assert_eq!(updated["hostname"], "db-1", "hostname 去除前後空白");
    assert_eq!(
        row(&list_ips(&pool, subnet_id, "").await, "10.0.0.1")["status"],
        "reservation"
    );

    // 保留可清除 hostname
    let (status, updated) = put_assignment(
        &pool,
        subnet_id,
        "10.0.0.1",
        json!({ "interface_id": interface_id, "purpose": "reservation", "hostname": null }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(updated["hostname"].is_null());

    // 手動設定不可填 hostname
    let (status, body) = put_assignment(
        &pool,
        subnet_id,
        "10.0.0.1",
        json!({
            "interface_id": interface_id,
            "purpose": "static",
            "hostname": "db-1"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "validation_error");
    assert_eq!(body["details"]["field"], "hostname");

    // 改回手動設定（未帶 hostname）即清除
    let (status, updated) = put_assignment(
        &pool,
        subnet_id,
        "10.0.0.1",
        json!({ "interface_id": interface_id, "purpose": "static" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(updated["hostname"].is_null());

    // 取消指派：位址回到「可用」（回應 200；未涉及 Kea 時 body 為空物件）
    let (status, body) = delete_assignment(&pool, subnet_id, "10.0.0.1").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({}));
    assert_eq!(assignment_count(&pool).await, 0);

    let page = list_ips(&pool, subnet_id, "").await;
    let released = row(&page, "10.0.0.1");
    assert_eq!(released["status"], "available");
    assert!(released["purpose"].is_null());
    assert!(released["assignment"].is_null());
    assert_eq!(get_asset(&pool, asset_id).await["assignments"], json!([]));

    // 再取消一次：404
    let (status, body) = delete_assignment(&pool, subnet_id, "10.0.0.1").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"], "not_found");
}

#[tokio::test]
async fn reservation_requires_interface_mac() {
    let pool = test_pool().await;
    let asset_id = create_asset(&pool, "無 MAC 設備", "機房 B").await;
    let interface_id = create_interface(&pool, asset_id, json!({ "name": "eth0" })).await;
    let subnet_id = create_subnet(&pool, json!({ "cidr": "10.0.0.0/29" })).await;

    // 無 MAC 不可保留
    let (status, body) = put_assignment(
        &pool,
        subnet_id,
        "10.0.0.1",
        json!({ "interface_id": interface_id, "purpose": "reservation", "hostname": "host-1" }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "validation_error");
    assert_eq!(body["details"]["field"], "purpose");
    assert!(
        body["message"]
            .as_str()
            .is_some_and(|message| message.contains("MAC")),
        "訊息說明保留需 MAC：{}",
        body["message"]
    );
    assert_eq!(assignment_count(&pool).await, 0, "驗證失敗不寫入");

    // 手動設定不受限
    let (status, _) = put_assignment(
        &pool,
        subnet_id,
        "10.0.0.1",
        json!({ "interface_id": interface_id, "purpose": "static" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn address_must_be_v4_host_inside_subnet_and_out_of_pool() {
    let pool = test_pool().await;
    let asset_id = create_asset(&pool, "測試主機", "機房 A").await;
    let interface_id = create_interface(&pool, asset_id, json!({ "name": "eth0" })).await;
    let subnet_id = create_subnet(
        &pool,
        json!({
            "cidr": "10.0.0.0/29",
            "gateway": "10.0.0.1",
            "pools": [{ "start_ip": "10.0.0.2", "end_ip": "10.0.0.3" }]
        }),
    )
    .await;

    let static_input = json!({ "interface_id": interface_id, "purpose": "static" });

    // 不在網段內
    let (status, body) = put_assignment(&pool, subnet_id, "10.0.1.1", static_input.clone()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["details"]["field"], "address");
    assert!(
        body["message"]
            .as_str()
            .is_some_and(|message| message.contains("10.0.0.0/29")),
        "訊息含網段：{}",
        body["message"]
    );

    // network 與 broadcast 不可指派
    for address in ["10.0.0.0", "10.0.0.7"] {
        let (status, body) = put_assignment(&pool, subnet_id, address, static_input.clone()).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{address} 應被阻擋");
        assert_eq!(body["details"]["field"], "address");
    }

    // pool 內不可指派
    let (status, body) = put_assignment(&pool, subnet_id, "10.0.0.2", static_input.clone()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["details"]["field"], "address");
    assert!(
        body["message"]
            .as_str()
            .is_some_and(|message| message.contains("池內")),
        "訊息說明池內不可指派：{}",
        body["message"]
    );

    // 非法位址
    let (status, body) = put_assignment(&pool, subnet_id, "abc", static_input.clone()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "abc 應被阻擋");
    assert_eq!(body["details"]["field"], "address");

    // gateway 可指派（僅標記，見 spec §7）
    let (status, _) = put_assignment(&pool, subnet_id, "10.0.0.1", static_input.clone()).await;
    assert_eq!(status, StatusCode::OK);

    // v6 位址指派到 v4 網段：地址族不符（v6 走登錄端點，見票 06）
    let (status, body) = put_assignment(&pool, subnet_id, "fd00::1", static_input.clone()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["details"]["field"], "address");
    assert!(
        body["message"]
            .as_str()
            .is_some_and(|message| message.contains("地址族")),
        "訊息說明地址族不符：{}",
        body["message"]
    );

    // v4 位址指派到 v6 網段：地址族不符
    let v6_subnet_id = create_subnet(&pool, json!({ "cidr": "fd00::/64" })).await;
    let (status, body) = put_assignment(&pool, v6_subnet_id, "10.0.0.1", static_input).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["details"]["field"], "address");
    assert!(
        body["message"]
            .as_str()
            .is_some_and(|message| message.contains("地址族")),
        "訊息說明地址族不符：{}",
        body["message"]
    );
}

#[tokio::test]
async fn excluded_address_is_blocked_through_subnet_endpoint() {
    let pool = test_pool().await;
    let asset_id = create_asset(&pool, "測試主機", "機房 A").await;
    let interface_id = create_interface(&pool, asset_id, json!({ "name": "eth0" })).await;
    let subnet_id = create_subnet(
        &pool,
        json!({
            "cidr": "10.0.0.0/29",
            "exclusions": [{ "start_ip": "10.0.0.4", "end_ip": "10.0.0.5" }]
        }),
    )
    .await;

    let static_input = json!({ "interface_id": interface_id, "purpose": "static" });

    // 排除範圍端點皆含：起點與終點皆不可新指派（見 spec §7、ADR-0020）
    for address in ["10.0.0.4", "10.0.0.5"] {
        let (status, body) = put_assignment(&pool, subnet_id, address, static_input.clone()).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{address} 應被阻擋");
        assert_eq!(body["error"], "validation_error");
        assert_eq!(body["details"]["field"], "address");
        assert!(
            body["message"]
                .as_str()
                .is_some_and(|message| message.contains("排除範圍")),
            "訊息說明排除範圍不可指派：{}",
            body["message"]
        );
    }
    assert_eq!(assignment_count(&pool).await, 0, "驗證失敗不寫入");

    // 範圍外仍可指派
    let (status, created) = put_assignment(&pool, subnet_id, "10.0.0.6", static_input).await;
    assert_eq!(status, StatusCode::OK, "範圍外可指派：{created}");
    assert_eq!(assignment_count(&pool).await, 1);
}

#[tokio::test]
async fn excluded_address_is_blocked_through_asset_endpoint() {
    let pool = test_pool().await;
    let asset_id = create_asset(&pool, "測試主機", "機房 A").await;
    let interface_id = create_interface(&pool, asset_id, json!({ "name": "eth0" })).await;
    let _subnet_id = create_subnet(
        &pool,
        json!({
            "cidr": "10.0.1.0/29",
            "exclusions": [{ "start_ip": "10.0.1.2", "end_ip": "10.0.1.2" }]
        }),
    )
    .await;

    // 未指派位址的新指派：即使帶 transfer=true（僅適用已指派位址）仍須阻擋
    for transfer in [false, true] {
        let (status, body) = send(
            &pool,
            Method::PUT,
            &format!("/api/v1/assets/{asset_id}/assignments"),
            Some(json!({
                "address": "10.0.1.2",
                "interface_id": interface_id,
                "purpose": "static",
                "transfer": transfer
            })),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "transfer={transfer} 應被阻擋：{body}"
        );
        assert_eq!(body["error"], "validation_error");
        assert_eq!(body["details"]["field"], "address");
        assert!(
            body["message"]
                .as_str()
                .is_some_and(|message| message.contains("排除範圍")),
            "訊息說明排除範圍不可指派：{}",
            body["message"]
        );
    }
    assert_eq!(assignment_count(&pool).await, 0, "驗證失敗不寫入");
}

#[tokio::test]
async fn later_exclusion_does_not_block_existing_assignment_update() {
    let pool = test_pool().await;
    let asset_id = create_asset(&pool, "資料庫主機", "機房 A").await;
    let interface_id = create_interface(
        &pool,
        asset_id,
        json!({ "name": "eth0", "mac": "AA:BB:CC:DD:EE:FF" }),
    )
    .await;
    let subnet_id = create_subnet(&pool, json!({ "cidr": "10.0.0.0/29" })).await;

    // 先指派
    let (status, _) = put_assignment(
        &pool,
        subnet_id,
        "10.0.0.2",
        json!({ "interface_id": interface_id, "purpose": "static" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // 後 PATCH 加排除範圍，覆蓋既有指派（不阻擋網段儲存）
    let (status, updated_subnet) = send(
        &pool,
        Method::PATCH,
        &format!("/api/v1/subnets/{subnet_id}"),
        Some(json!({
            "exclusions": [{ "start_ip": "10.0.0.2", "end_ip": "10.0.0.3" }]
        })),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "加排除範圍不阻擋網段儲存：{updated_subnet}"
    );
    assert_eq!(updated_subnet["exclusions"][0]["start_ip"], "10.0.0.2");

    // 同介面同位址更新用途：不重驗排除範圍 → 200，以語意衝突標記提示
    let (status, updated) = put_assignment(
        &pool,
        subnet_id,
        "10.0.0.2",
        json!({
            "interface_id": interface_id,
            "purpose": "reservation",
            "hostname": "db-1"
        }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "既有指派更新不因排除範圍阻擋：{updated}"
    );
    assert_eq!(updated["purpose"], "reservation");
    assert_eq!(updated["hostname"], "db-1");
    assert_eq!(
        updated["warnings"][0]["code"], "IpInExcludedRange",
        "以語意衝突標記呈現：{}",
        updated["warnings"]
    );
    assert!(
        updated["warnings"][0]["message"]
            .as_str()
            .is_some_and(|message| message.contains("排除範圍")),
        "警示說明排除範圍：{}",
        updated["warnings"]
    );
    assert_eq!(assignment_count(&pool).await, 1, "更新既有列而非新增");
}

#[tokio::test]
async fn v6_registration_is_unaffected_by_exclusion_block() {
    let pool = test_pool().await;
    let asset_id = create_asset(&pool, "v6 主機", "機房 C").await;
    let interface_id = create_interface(&pool, asset_id, json!({ "name": "eth0" })).await;
    let subnet_id = create_subnet(&pool, json!({ "cidr": "fd00::/64" })).await;

    // v6 登錄（新增即指派）仍成功；排除範圍為 v4 限定（見 spec §7）
    let (status, created) = send(
        &pool,
        Method::POST,
        &format!("/api/v1/subnets/{subnet_id}/ips"),
        Some(json!({ "address": "fd00::5", "interface_id": interface_id })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "v6 登錄應成功：{created}");
    assert_eq!(created["subnet_id"], subnet_id);
    assert_eq!(created["address"], "fd00::5");
    assert_eq!(created["interface_id"], interface_id);
    assert_eq!(created["purpose"], "static");
    assert_eq!(assignment_count(&pool).await, 1);
}

#[tokio::test]
async fn duplicate_address_and_same_interface_rules() {
    let pool = test_pool().await;
    let first_asset = create_asset(&pool, "設備一", "機房 A").await;
    let second_asset = create_asset(&pool, "設備二", "機房 A").await;
    let first_interface = create_interface(&pool, first_asset, json!({ "name": "eth0" })).await;
    let second_interface = create_interface(&pool, second_asset, json!({ "name": "eth0" })).await;
    let first_subnet = create_subnet(&pool, json!({ "cidr": "10.0.0.0/29" })).await;
    let second_subnet = create_subnet(&pool, json!({ "cidr": "10.0.1.0/29" })).await;

    // 第一位址指派給介面一
    let (status, _) = put_assignment(
        &pool,
        first_subnet,
        "10.0.0.1",
        json!({ "interface_id": first_interface, "purpose": "static" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // 同一 Subnet 同一位址不得重複指派
    let (status, body) = put_assignment(
        &pool,
        first_subnet,
        "10.0.0.1",
        json!({ "interface_id": second_interface, "purpose": "static" }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["details"]["field"], "address");
    assert_eq!(
        body["details"]["interface_id"], first_interface,
        "附目前指派介面"
    );

    // 同一介面在同一 Subnet 至多一個位址
    let (status, body) = put_assignment(
        &pool,
        first_subnet,
        "10.0.0.2",
        json!({ "interface_id": first_interface, "purpose": "static" }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["details"]["field"], "interface_id");
    assert_eq!(body["details"]["existing_address"], "10.0.0.1");

    // 跨 Subnet 可各有一個
    let (status, _) = put_assignment(
        &pool,
        second_subnet,
        "10.0.1.2",
        json!({ "interface_id": second_interface, "purpose": "static" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = put_assignment(
        &pool,
        second_subnet,
        "10.0.1.1",
        json!({ "interface_id": first_interface, "purpose": "static" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "同一介面跨網段可各有位址");

    // 跨 Subnet 後仍受兩條唯一性限制
    let (status, body) = put_assignment(
        &pool,
        second_subnet,
        "10.0.1.1",
        json!({ "interface_id": second_interface, "purpose": "static" }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["details"]["field"], "address");

    let (status, body) = put_assignment(
        &pool,
        second_subnet,
        "10.0.1.3",
        json!({ "interface_id": first_interface, "purpose": "static" }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["details"]["field"], "interface_id");
    assert_eq!(body["details"]["existing_address"], "10.0.1.1");

    assert_eq!(assignment_count(&pool).await, 3);
}

#[tokio::test]
async fn unique_constraints_are_enforced_in_database() {
    let pool = test_pool().await;
    let first_asset = create_asset(&pool, "設備一", "機房 A").await;
    let second_asset = create_asset(&pool, "設備二", "機房 A").await;
    let first_interface = create_interface(&pool, first_asset, json!({ "name": "eth0" })).await;
    let second_interface = create_interface(&pool, second_asset, json!({ "name": "eth0" })).await;
    let first_subnet = create_subnet(&pool, json!({ "cidr": "10.0.0.0/29" })).await;
    let second_subnet = create_subnet(&pool, json!({ "cidr": "10.0.1.0/29" })).await;

    let insert = |subnet_id: i64, address: &str, interface_id: i64, purpose: &str| {
        let pool = pool.clone();
        let address = address.to_string();
        let purpose = purpose.to_string();
        async move {
            sqlx::query(
                "INSERT INTO ip_assignments (subnet_id, address, interface_id, purpose)
                 VALUES (?, ?, ?, ?)",
            )
            .bind(subnet_id)
            .bind(address)
            .bind(interface_id)
            .bind(purpose)
            .execute(&pool)
            .await
        }
    };

    // 合法寫入
    insert(first_subnet, "10.0.0.1", first_interface, "static")
        .await
        .expect("合法指派");

    // UNIQUE (subnet_id, address)
    assert!(
        insert(first_subnet, "10.0.0.1", second_interface, "static")
            .await
            .is_err(),
        "同一網段同一位址不可重複"
    );

    // UNIQUE (interface_id, subnet_id)
    assert!(
        insert(first_subnet, "10.0.0.2", first_interface, "static")
            .await
            .is_err(),
        "同一介面同一網段僅能一筆"
    );

    // 跨網段可各有一筆
    insert(second_subnet, "10.0.0.1", first_interface, "reservation")
        .await
        .expect("跨網段可各有一筆");

    // purpose CHECK
    assert!(
        insert(second_subnet, "10.0.1.5", second_interface, "dhcp")
            .await
            .is_err(),
        "用途僅限 static／reservation"
    );
}

#[tokio::test]
async fn deleting_interface_cascades_assignments() {
    let pool = test_pool().await;
    let asset_id = create_asset(&pool, "雙網卡主機", "機房 A").await;
    let first_interface = create_interface(
        &pool,
        asset_id,
        json!({ "name": "eth0", "mac": "aa:bb:cc:dd:ee:01" }),
    )
    .await;
    let second_interface = create_interface(
        &pool,
        asset_id,
        json!({ "name": "eth1", "mac": "aa:bb:cc:dd:ee:02" }),
    )
    .await;
    let subnet_id = create_subnet(&pool, json!({ "cidr": "10.0.0.0/29" })).await;

    let (status, _) = put_assignment(
        &pool,
        subnet_id,
        "10.0.0.1",
        json!({ "interface_id": first_interface, "purpose": "static" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = put_assignment(
        &pool,
        subnet_id,
        "10.0.0.2",
        json!({ "interface_id": second_interface, "purpose": "reservation" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(assignment_count(&pool).await, 2);

    // 刪除介面 → 其指派連動刪除（ON DELETE CASCADE）
    let (status, _) = send(
        &pool,
        Method::DELETE,
        &format!("/api/v1/interfaces/{first_interface}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(assignment_count(&pool).await, 1);

    let page = list_ips(&pool, subnet_id, "").await;
    assert_eq!(row(&page, "10.0.0.1")["status"], "available");
    assert_eq!(row(&page, "10.0.0.2")["status"], "reservation");

    let detail = get_asset(&pool, asset_id).await;
    let assignments = detail["assignments"]
        .as_array()
        .expect("assignments 為陣列");
    assert_eq!(assignments.len(), 1);
    assert_eq!(assignments[0]["interface_id"], second_interface);

    // 刪除資產 → 剩餘介面與指派一併連動刪除（數量顯示見票 08）
    let (status, _) = send(
        &pool,
        Method::DELETE,
        &format!("/api/v1/assets/{asset_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(assignment_count(&pool).await, 0);
}

#[tokio::test]
async fn list_search_and_status_filter_cover_assignment_target() {
    let pool = test_pool().await;
    let first_asset = create_asset(&pool, "資料庫主機", "機房 A").await;
    let second_asset = create_asset(&pool, "印表機", "機房 B").await;
    let first_interface = create_interface(
        &pool,
        first_asset,
        json!({ "name": "eth0", "mac": "aa:bb:cc:dd:ee:01" }),
    )
    .await;
    let second_interface = create_interface(
        &pool,
        second_asset,
        json!({ "name": "wlan0", "mac": "11:22:33:44:55:66" }),
    )
    .await;
    let subnet_id = create_subnet(&pool, json!({ "cidr": "10.0.0.0/29" })).await;

    let (status, _) = put_assignment(
        &pool,
        subnet_id,
        "10.0.0.1",
        json!({ "interface_id": first_interface, "purpose": "static" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = put_assignment(
        &pool,
        subnet_id,
        "10.0.0.2",
        json!({
            "interface_id": second_interface,
            "purpose": "reservation",
            "hostname": "printer-1"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // 關鍵字可比對資產描述、介面名稱與 MAC（不分大小寫）
    for (query, address) in [
        ("資料庫", "10.0.0.1"),
        ("ETH0", "10.0.0.1"),
        ("bb:cc", "10.0.0.1"),
        ("印表機", "10.0.0.2"),
        ("WLAN0", "10.0.0.2"),
        ("55:66", "10.0.0.2"),
    ] {
        let page = list_ips(&pool, subnet_id, &format!("?q={query}")).await;
        assert_eq!(page["total"], 1, "q={query}");
        assert_eq!(page["items"][0]["address"], address, "q={query}");
    }

    // 狀態／用途篩選
    let page = list_ips(&pool, subnet_id, "?status=static").await;
    assert_eq!(page["total"], 1);
    assert_eq!(page["items"][0]["address"], "10.0.0.1");
    assert!(
        page["items"][0]["assignment"]["asset_property_no"].is_null(),
        "資產無財產編號時為 null（見票 15）"
    );

    let page = list_ips(&pool, subnet_id, "?status=reservation").await;
    assert_eq!(page["total"], 1);
    assert_eq!(page["items"][0]["address"], "10.0.0.2");
    assert_eq!(
        page["items"][0]["assignment"]["hostname"], "printer-1",
        "保留的 hostname 隨指派對象顯示"
    );

    let page = list_ips(&pool, subnet_id, "?status=available").await;
    assert_eq!(page["total"], 4, "/29 六個 host 扣掉兩個指派");

    let page = list_ips(&pool, subnet_id, "?status=in_pool").await;
    assert_eq!(page["total"], 0, "無 pool 設定");

    // 關鍵字與狀態併用
    let page = list_ips(&pool, subnet_id, "?q=資料庫&status=reservation").await;
    assert_eq!(page["total"], 0);

    // 未知狀態值：400
    let (status, body) = send(
        &pool,
        Method::GET,
        &format!("/api/v1/subnets/{subnet_id}/ips?status=dhcp"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "validation_error");
    assert_eq!(body["details"]["field"], "status");
}

#[tokio::test]
async fn unknown_resources_and_bad_input_are_reported() {
    let pool = test_pool().await;
    let asset_id = create_asset(&pool, "測試主機", "機房 A").await;
    let interface_id = create_interface(&pool, asset_id, json!({ "name": "eth0" })).await;
    let subnet_id = create_subnet(&pool, json!({ "cidr": "10.0.0.0/29" })).await;

    // 不存在的網段
    let (status, body) = put_assignment(
        &pool,
        999,
        "10.0.0.1",
        json!({ "interface_id": interface_id, "purpose": "static" }),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"], "not_found");

    // 不存在的介面
    let (status, body) = put_assignment(
        &pool,
        subnet_id,
        "10.0.0.1",
        json!({ "interface_id": 999, "purpose": "static" }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["details"]["field"], "interface_id");

    // 缺介面、缺用途、未知用途
    let (status, body) =
        put_assignment(&pool, subnet_id, "10.0.0.1", json!({ "purpose": "static" })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["details"]["field"], "interface_id");

    let (status, body) = put_assignment(
        &pool,
        subnet_id,
        "10.0.0.1",
        json!({ "interface_id": interface_id }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["details"]["field"], "purpose");

    let (status, body) = put_assignment(
        &pool,
        subnet_id,
        "10.0.0.1",
        json!({ "interface_id": interface_id, "purpose": "dhcp" }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["details"]["field"], "purpose");

    // 取消：不存在的網段、不存在的指派、非法位址
    let (status, body) = delete_assignment(&pool, 999, "10.0.0.1").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["message"], "找不到網段");

    let (status, body) = delete_assignment(&pool, subnet_id, "10.0.0.5").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["message"], "找不到指派");

    let (status, body) = delete_assignment(&pool, subnet_id, "abc").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["details"]["field"], "address");
}
