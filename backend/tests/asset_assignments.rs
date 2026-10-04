//! 資產端指派整合測試（見票 10、ADR-0007）：指派、確認後移轉（v4／v6）、
//! 結構規則、不信任情境，以及既有網段端點仍阻擋換介面的回歸。

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

/// 資產端指派（不檢查狀態碼，供各測試自行斷言）。
async fn put_asset_assignment(
    pool: &SqlitePool,
    asset_id: i64,
    body: Value,
) -> (StatusCode, Value) {
    send(
        pool,
        Method::PUT,
        &format!("/api/v1/assets/{asset_id}/assignments"),
        Some(body),
    )
    .await
}

/// 既有網段端點指派（回歸用；不檢查狀態碼）。
async fn put_subnet_assignment(
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

/// 讀取某網段的 IP 清單並斷言成功。
async fn list_ips(pool: &SqlitePool, subnet_id: i64) -> Value {
    let (status, json) = send(
        pool,
        Method::GET,
        &format!("/api/v1/subnets/{subnet_id}/ips"),
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

/// 指派列總數（直接查資料庫）。
async fn assignment_count(pool: &SqlitePool) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM ip_assignments")
        .fetch_one(pool)
        .await
        .expect("查詢指派筆數")
}

/// 資產詳情中的指派位址清單。
fn addresses_of(detail: &Value) -> Vec<String> {
    detail["assignments"]
        .as_array()
        .expect("assignments 為陣列")
        .iter()
        .map(|item| {
            item["address"]
                .as_str()
                .expect("指派含 address")
                .to_string()
        })
        .collect()
}

#[tokio::test]
async fn asset_side_assignment_creates_and_updates_in_place() {
    let pool = test_pool().await;
    let asset_id = create_asset(&pool, "資料庫主機", "機房 A").await;
    let interface_id = create_interface(
        &pool,
        asset_id,
        json!({ "name": "eth0", "mac": "aa:bb:cc:dd:ee:01" }),
    )
    .await;
    let subnet_id = create_subnet(&pool, json!({ "cidr": "10.0.0.0/29" })).await;

    // 新指派：v4 host、非池內
    let (status, created) = put_asset_assignment(
        &pool,
        asset_id,
        json!({
            "address": "10.0.0.1",
            "interface_id": interface_id,
            "purpose": "static",
            "hostname": null,
            "transfer": false
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "指派應成功：{created}");
    assert_eq!(created["subnet_id"], subnet_id);
    assert_eq!(created["address"], "10.0.0.1");
    assert_eq!(created["interface_id"], interface_id);
    assert_eq!(created["purpose"], "static");
    assert!(created["hostname"].is_null());
    assert_eq!(created["warnings"], json!([]));
    assert_eq!(created["transferred"], false, "新指派非移轉");

    // 資產詳情可見（唯讀顯示用）
    let detail = get_asset(&pool, asset_id).await;
    assert_eq!(addresses_of(&detail), vec!["10.0.0.1"]);
    assert_eq!(detail["assignments"][0]["subnet_cidr"], "10.0.0.0/29");
    assert_eq!(detail["assignments"][0]["interface_id"], interface_id);

    // 同一介面同位址＝更新用途／hostname（transferred 為 false、列 id 不變）
    let original_id = created["id"].as_i64().expect("回應含 id");
    let (status, updated) = put_asset_assignment(
        &pool,
        asset_id,
        json!({
            "address": "10.0.0.1",
            "interface_id": interface_id,
            "purpose": "reservation",
            "hostname": " db-1 ",
            "transfer": false
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "改用途應成功：{updated}");
    assert_eq!(updated["id"], original_id, "更新不重建列");
    assert_eq!(updated["purpose"], "reservation");
    assert_eq!(updated["hostname"], "db-1", "hostname 去除前後空白");
    assert_eq!(updated["transferred"], false);
    assert_eq!(assignment_count(&pool).await, 1);
}

#[tokio::test]
async fn assigned_elsewhere_blocks_then_transfers_atomically() {
    let pool = test_pool().await;
    let first_asset = create_asset_with_property_no(&pool, "P-100", "資料庫主機", "機房 A").await;
    let second_asset = create_asset(&pool, "印表機", "機房 B").await;
    let third_asset = create_asset(&pool, "測試機", "機房 C").await;
    let first_interface = create_interface(
        &pool,
        first_asset,
        json!({ "name": "eth0", "mac": "aa:bb:cc:dd:ee:01" }),
    )
    .await;
    let second_interface = create_interface(
        &pool,
        second_asset,
        json!({ "name": "wlan0", "mac": "aa:bb:cc:dd:ee:02" }),
    )
    .await;
    let third_interface = create_interface(
        &pool,
        third_asset,
        json!({ "name": "eth0", "mac": "aa:bb:cc:dd:ee:03" }),
    )
    .await;
    let subnet_id = create_subnet(&pool, json!({ "cidr": "10.0.0.0/29" })).await;

    // 先指派兩個位址：10.0.0.1 給第一台、10.0.0.2 給第三台
    let (status, first) = put_asset_assignment(
        &pool,
        first_asset,
        json!({
            "address": "10.0.0.1",
            "interface_id": first_interface,
            "purpose": "static",
            "transfer": false
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "初始指派應成功：{first}");
    let first_id = first["id"].as_i64().expect("回應含 id");

    let (status, _) = put_asset_assignment(
        &pool,
        third_asset,
        json!({
            "address": "10.0.0.2",
            "interface_id": third_interface,
            "purpose": "static",
            "transfer": false
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // IP 清單指派對象含財產編號（見票 15）
    let page = list_ips(&pool, subnet_id).await;
    assert_eq!(
        row(&page, "10.0.0.1")["assignment"]["asset_property_no"],
        "P-100"
    );

    // 已指派給其他介面：transfer 未提供（預設 false）即阻擋
    let (status, body) = put_asset_assignment(
        &pool,
        second_asset,
        json!({
            "address": "10.0.0.1",
            "interface_id": second_interface,
            "purpose": "static"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "validation_error");
    assert_eq!(body["details"]["reason"], "address_assigned_elsewhere");

    // transfer=false：附目前指派對象完整欄位
    let (status, body) = put_asset_assignment(
        &pool,
        second_asset,
        json!({
            "address": "10.0.0.1",
            "interface_id": second_interface,
            "purpose": "static",
            "transfer": false
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "validation_error");
    assert_eq!(body["details"]["field"], "address");
    assert_eq!(body["details"]["reason"], "address_assigned_elsewhere");
    assert_eq!(body["details"]["subnet_id"], subnet_id);
    assert_eq!(body["details"]["subnet_cidr"], "10.0.0.0/29");
    assert_eq!(body["details"]["asset_id"], first_asset);
    assert_eq!(body["details"]["asset_property_no"], "P-100");
    assert_eq!(body["details"]["asset_description"], "資料庫主機");
    assert_eq!(body["details"]["asset_location"], "機房 A");
    assert_eq!(body["details"]["interface_id"], first_interface);
    assert_eq!(body["details"]["interface_name"], "eth0");
    assert_eq!(body["details"]["mac"], "aa:bb:cc:dd:ee:01");

    // 阻擋時原指派與其他列完全不變
    assert_eq!(
        addresses_of(&get_asset(&pool, first_asset).await),
        vec!["10.0.0.1"]
    );
    assert_eq!(
        addresses_of(&get_asset(&pool, second_asset).await),
        Vec::<String>::new()
    );
    assert_eq!(assignment_count(&pool).await, 2);

    // transfer=true：原子移轉
    let (status, transferred) = put_asset_assignment(
        &pool,
        second_asset,
        json!({
            "address": "10.0.0.1",
            "interface_id": second_interface,
            "purpose": "static",
            "transfer": true
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "移轉應成功：{transferred}");
    assert_eq!(transferred["transferred"], true);
    assert_eq!(transferred["interface_id"], second_interface);
    assert_eq!(transferred["address"], "10.0.0.1");
    assert_ne!(
        transferred["id"], first_id,
        "移轉為取消＋重新指派，列 id 改變（不留歷程）"
    );
    assert_eq!(assignment_count(&pool).await, 2, "移轉不增減總數");

    // 舊介面指派消失、新介面指派存在
    assert_eq!(
        addresses_of(&get_asset(&pool, first_asset).await),
        Vec::<String>::new()
    );
    assert_eq!(
        addresses_of(&get_asset(&pool, second_asset).await),
        vec!["10.0.0.1"]
    );
    assert_eq!(
        addresses_of(&get_asset(&pool, third_asset).await),
        vec!["10.0.0.2"],
        "原網段其他指派不受影響"
    );

    let page = list_ips(&pool, subnet_id).await;
    assert_eq!(
        row(&page, "10.0.0.1")["assignment"]["interface_id"],
        second_interface
    );
    assert!(
        row(&page, "10.0.0.1")["assignment"]["asset_property_no"].is_null(),
        "移轉後對象（印表機）無財產編號為 null"
    );
    assert_eq!(
        row(&page, "10.0.0.2")["assignment"]["interface_id"],
        third_interface
    );
}

#[tokio::test]
async fn asset_side_structural_rules_are_enforced() {
    let pool = test_pool().await;
    let asset_id = create_asset(&pool, "測試主機", "機房 A").await;
    let other_asset = create_asset(&pool, "其他主機", "機房 B").await;
    let interface_id = create_interface(&pool, asset_id, json!({ "name": "eth0" })).await;
    let other_interface = create_interface(
        &pool,
        other_asset,
        json!({ "name": "eth0", "mac": "aa:bb:cc:dd:ee:09" }),
    )
    .await;
    let subnet_id = create_subnet(
        &pool,
        json!({
            "cidr": "10.0.0.0/29",
            "pools": [{ "start_ip": "10.0.0.2", "end_ip": "10.0.0.3" }]
        }),
    )
    .await;

    // 保留需介面有 MAC（介面無 MAC）
    let (status, body) = put_asset_assignment(
        &pool,
        asset_id,
        json!({
            "address": "10.0.0.1",
            "interface_id": interface_id,
            "purpose": "reservation",
            "hostname": "host-1",
            "transfer": false
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["details"]["field"], "purpose");
    assert!(
        body["message"]
            .as_str()
            .is_some_and(|message| message.contains("MAC")),
        "訊息說明保留需 MAC：{}",
        body["message"]
    );

    // 介面不屬於該資產
    let (status, body) = put_asset_assignment(
        &pool,
        asset_id,
        json!({
            "address": "10.0.0.1",
            "interface_id": other_interface,
            "purpose": "static",
            "transfer": false
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["details"]["field"], "interface_id");
    assert!(
        body["message"]
            .as_str()
            .is_some_and(|message| message.contains("不屬於")),
        "訊息說明介面不屬資產：{}",
        body["message"]
    );

    // 位址不在任何網段內
    let (status, body) = put_asset_assignment(
        &pool,
        asset_id,
        json!({
            "address": "192.168.99.1",
            "interface_id": interface_id,
            "purpose": "static",
            "transfer": false
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["details"]["field"], "address");
    assert_eq!(body["details"]["reason"], "no_subnet");

    // v4 pool 內不可新指派
    let (status, body) = put_asset_assignment(
        &pool,
        asset_id,
        json!({
            "address": "10.0.0.2",
            "interface_id": interface_id,
            "purpose": "static",
            "transfer": false
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["details"]["field"], "address");
    assert!(
        body["message"]
            .as_str()
            .is_some_and(|message| message.contains("池內")),
        "訊息說明池內不可指派：{}",
        body["message"]
    );

    // network／broadcast 不可指派
    for address in ["10.0.0.0", "10.0.0.7"] {
        let (status, body) = put_asset_assignment(
            &pool,
            asset_id,
            json!({
                "address": address,
                "interface_id": interface_id,
                "purpose": "static",
                "transfer": false
            }),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{address} 應被阻擋");
        assert_eq!(body["details"]["field"], "address");
    }

    // 目標介面在同一網段已有其他位址（不因移轉而放寬）
    let (status, _) = put_asset_assignment(
        &pool,
        asset_id,
        json!({
            "address": "10.0.0.1",
            "interface_id": interface_id,
            "purpose": "static",
            "transfer": false
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = put_asset_assignment(
        &pool,
        asset_id,
        json!({
            "address": "10.0.0.4",
            "interface_id": interface_id,
            "purpose": "static",
            "transfer": false
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["details"]["field"], "interface_id");
    assert_eq!(body["details"]["existing_address"], "10.0.0.1");

    // 不存在的資產：404；不存在的介面：400
    let (status, body) = put_asset_assignment(
        &pool,
        9999,
        json!({
            "address": "10.0.0.4",
            "interface_id": interface_id,
            "purpose": "static",
            "transfer": false
        }),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"], "not_found");

    let (status, body) = put_asset_assignment(
        &pool,
        asset_id,
        json!({
            "address": "10.0.0.4",
            "interface_id": 9999,
            "purpose": "static",
            "transfer": false
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["details"]["field"], "interface_id");

    // 非法位址
    let (status, body) = put_asset_assignment(
        &pool,
        asset_id,
        json!({
            "address": "abc",
            "interface_id": interface_id,
            "purpose": "static",
            "transfer": false
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["details"]["field"], "address");

    // 以上阻擋皆未寫入；既有指派仍在
    let page = list_ips(&pool, subnet_id).await;
    assert_eq!(
        row(&page, "10.0.0.1")["assignment"]["interface_id"],
        interface_id
    );
}

#[tokio::test]
async fn asset_side_v6_registration_and_transfer() {
    let pool = test_pool().await;
    let first_asset = create_asset(&pool, "伺服器", "機房 A").await;
    let second_asset = create_asset(&pool, "工作站", "機房 B").await;
    let first_interface = create_interface(&pool, first_asset, json!({ "name": "eth0" })).await;
    let second_interface = create_interface(&pool, second_asset, json!({ "name": "eth1" })).await;
    let subnet_id = create_subnet(&pool, json!({ "cidr": "fd00::/64" })).await;

    // v6 新登錄（建立即指派；用途固定 static）
    let (status, created) = put_asset_assignment(
        &pool,
        first_asset,
        json!({
            "address": "fd00::5",
            "interface_id": first_interface,
            "purpose": "static",
            "transfer": false
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "v6 登錄應成功：{created}");
    assert_eq!(created["address"], "fd00::5");
    assert_eq!(created["subnet_id"], subnet_id);
    assert_eq!(created["transferred"], false);
    assert_eq!(
        addresses_of(&get_asset(&pool, first_asset).await),
        vec!["fd00::5"]
    );

    // 已登錄給其他介面：transfer=false 擋、附目前對象
    let (status, body) = put_asset_assignment(
        &pool,
        second_asset,
        json!({
            "address": "fd00::5",
            "interface_id": second_interface,
            "purpose": "static",
            "transfer": false
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["details"]["reason"], "address_assigned_elsewhere");
    assert_eq!(body["details"]["subnet_cidr"], "fd00::/64");
    assert_eq!(body["details"]["interface_id"], first_interface);
    assert!(
        body["details"]["asset_property_no"].is_null(),
        "無財產編號為 null（見票 15）"
    );

    // transfer=true：移轉
    let (status, transferred) = put_asset_assignment(
        &pool,
        second_asset,
        json!({
            "address": "fd00::5",
            "interface_id": second_interface,
            "purpose": "static",
            "transfer": true
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "v6 移轉應成功：{transferred}");
    assert_eq!(transferred["transferred"], true);
    assert_eq!(
        addresses_of(&get_asset(&pool, first_asset).await),
        Vec::<String>::new()
    );
    assert_eq!(
        addresses_of(&get_asset(&pool, second_asset).await),
        vec!["fd00::5"]
    );

    // v6 用途固定 static
    let (status, body) = put_asset_assignment(
        &pool,
        first_asset,
        json!({
            "address": "fd00::6",
            "interface_id": first_interface,
            "purpose": "reservation",
            "hostname": "host-1",
            "transfer": false
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["details"]["field"], "purpose");

    // v6 允許 network 位址（無 host 扣除）
    let (status, created) = put_asset_assignment(
        &pool,
        first_asset,
        json!({
            "address": "fd00::",
            "interface_id": first_interface,
            "purpose": "static",
            "transfer": false
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "v6 network 位址可登錄：{created}");

    // 不在任何網段內
    let (status, body) = put_asset_assignment(
        &pool,
        first_asset,
        json!({
            "address": "fd01::1",
            "interface_id": first_interface,
            "purpose": "static",
            "transfer": false
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["details"]["reason"], "no_subnet");
}

#[tokio::test]
async fn asset_side_assignment_reports_warnings() {
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
        json!({ "name": "eth1", "mac": "aa:bb:cc:dd:ee:01" }),
    )
    .await;
    let subnet_id = create_subnet(&pool, json!({ "cidr": "10.0.0.0/29" })).await;

    let (status, first) = put_asset_assignment(
        &pool,
        asset_id,
        json!({
            "address": "10.0.0.1",
            "interface_id": first_interface,
            "purpose": "reservation",
            "transfer": false
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(first["warnings"], json!([]), "第一筆保留無衝突");

    // 同 MAC 第二筆保留：回應附 DuplicateHwAddress 警示（僅提示、不阻擋）
    let (status, second) = put_asset_assignment(
        &pool,
        asset_id,
        json!({
            "address": "10.0.0.2",
            "interface_id": second_interface,
            "purpose": "reservation",
            "transfer": false
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "警示不阻擋儲存：{second}");
    let warnings = second["warnings"].as_array().expect("warnings 為陣列");
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0]["code"], "DuplicateHwAddress");
    assert!(
        warnings[0]["message"]
            .as_str()
            .is_some_and(|message| message.contains("aa:bb:cc:dd:ee:01")),
        "警示含 MAC：{}",
        warnings[0]["message"]
    );

    // IP 清單同步標記衝突
    let page = list_ips(&pool, subnet_id).await;
    assert_eq!(
        row(&page, "10.0.0.1")["conflicts"],
        json!(["DuplicateHwAddress"])
    );
    assert_eq!(
        row(&page, "10.0.0.2")["conflicts"],
        json!(["DuplicateHwAddress"])
    );
}

#[tokio::test]
async fn subnet_endpoint_still_blocks_other_interface() {
    let pool = test_pool().await;
    let first_asset = create_asset(&pool, "設備一", "機房 A").await;
    let second_asset = create_asset(&pool, "設備二", "機房 A").await;
    let first_interface = create_interface(&pool, first_asset, json!({ "name": "eth0" })).await;
    let second_interface = create_interface(&pool, second_asset, json!({ "name": "eth0" })).await;
    let subnet_id = create_subnet(&pool, json!({ "cidr": "10.0.0.0/29" })).await;

    let (status, _) = put_subnet_assignment(
        &pool,
        subnet_id,
        "10.0.0.1",
        json!({ "interface_id": first_interface, "purpose": "static" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // 既有端點維持「已指派給其他介面即阻擋」，不提供移轉語意
    let (status, body) = put_subnet_assignment(
        &pool,
        subnet_id,
        "10.0.0.1",
        json!({ "interface_id": second_interface, "purpose": "static" }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "validation_error");
    assert_eq!(body["details"]["field"], "address");
    assert_eq!(body["details"]["interface_id"], first_interface);
    assert!(
        body["message"]
            .as_str()
            .is_some_and(|message| message.contains("已被指派給其他介面")),
        "訊息維持原本阻擋語意：{}",
        body["message"]
    );
    assert!(
        body["details"]["reason"].is_null(),
        "既有端點不回 address_assigned_elsewhere"
    );
    assert_eq!(assignment_count(&pool).await, 1);
}
