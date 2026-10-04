//! v6 位址登錄制整合測試：登錄即指派、清單、取消與結構規則（見票 06）。

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

/// v6 登錄（不檢查狀態碼，供各測試自行斷言）。
async fn register_ip(pool: &SqlitePool, subnet_id: i64, body: Value) -> (StatusCode, Value) {
    send(
        pool,
        Method::POST,
        &format!("/api/v1/subnets/{subnet_id}/ips"),
        Some(body),
    )
    .await
}

/// 指派／改用途（不檢查狀態碼）。
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

/// 取出回應列中的位址字串。
fn addresses(page: &Value) -> Vec<&str> {
    page["items"]
        .as_array()
        .expect("items 為陣列")
        .iter()
        .map(|item| item["address"].as_str().expect("address 為字串"))
        .collect()
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

#[tokio::test]
async fn register_creates_static_assignment_and_lists_only_registered() {
    let pool = test_pool().await;
    let asset_id = create_asset(&pool, "資料庫主機", "機房 A").await;
    let first_interface = create_interface(&pool, asset_id, json!({ "name": "eth0" })).await;
    let second_interface = create_interface(&pool, asset_id, json!({ "name": "eth1" })).await;
    let subnet_id =
        create_subnet(&pool, json!({ "cidr": "fd00::/64", "gateway": "fd00::1" })).await;

    // 登錄前：清單為空（v6 不枚舉空閒位址）
    let page = list_ips(&pool, subnet_id, "").await;
    assert_eq!(page["total"], 0);
    assert!(addresses(&page).is_empty());

    // 新增即指派（201）：用途固定手動設定、無 hostname
    let (status, created) = register_ip(
        &pool,
        subnet_id,
        json!({ "address": "fd00::10", "interface_id": first_interface }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "登錄應成功：{created}");
    assert_eq!(created["subnet_id"], subnet_id);
    assert_eq!(created["address"], "fd00::10");
    assert_eq!(created["interface_id"], first_interface);
    assert_eq!(created["purpose"], "static", "v6 用途固定手動設定");
    assert!(created["hostname"].is_null());

    // 清單僅含登錄位址；列欄位完整
    let page = list_ips(&pool, subnet_id, "").await;
    assert_eq!(page["total"], 1);
    let registered = row(&page, "fd00::10");
    assert_eq!(registered["status"], "static");
    assert_eq!(registered["purpose"], "static");
    assert_eq!(registered["in_pool"], false, "v6 無 pool 概念");
    assert_eq!(registered["conflicts"], json!([]), "衝突欄位預留票 07");
    assert_eq!(registered["assignment"]["asset_id"], asset_id);
    assert_eq!(registered["assignment"]["asset_description"], "資料庫主機");
    assert_eq!(registered["assignment"]["asset_location"], "機房 A");
    assert_eq!(registered["assignment"]["interface_id"], first_interface);
    assert_eq!(registered["assignment"]["interface_name"], "eth0");

    // gateway 位址仍可登錄（僅不再標記於清單列，見 spec §7、票 17）
    let (status, _) = register_ip(
        &pool,
        subnet_id,
        json!({ "address": "fd00::1", "interface_id": second_interface }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let page = list_ips(&pool, subnet_id, "").await;
    assert_eq!(page["total"], 2);
    assert_eq!(row(&page, "fd00::1")["status"], "static");

    // 資產詳情含 v6 指派（含網段資訊）
    let detail = get_asset(&pool, asset_id).await;
    let assignments = detail["assignments"]
        .as_array()
        .expect("assignments 為陣列");
    assert_eq!(assignments.len(), 2);
    assert_eq!(assignments[0]["address"], "fd00::10");
    assert_eq!(assignments[0]["subnet_cidr"], "fd00::/64");
    assert_eq!(assignments[0]["purpose"], "static");
    assert_eq!(assignments[1]["address"], "fd00::1");
}

#[tokio::test]
async fn register_validates_address_and_interface_input() {
    let pool = test_pool().await;
    let asset_id = create_asset(&pool, "測試主機", "機房 A").await;
    let interface_id = create_interface(&pool, asset_id, json!({ "name": "eth0" })).await;
    let subnet_id = create_subnet(&pool, json!({ "cidr": "fd00::/64" })).await;

    // v4 網段呼叫登錄端點：400（v4 走指派端點）
    let v4_subnet_id = create_subnet(&pool, json!({ "cidr": "10.0.0.0/29" })).await;
    let (status, body) = register_ip(
        &pool,
        v4_subnet_id,
        json!({ "address": "fd00::10", "interface_id": interface_id }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "validation_error");
    assert!(
        body["message"]
            .as_str()
            .is_some_and(|message| message.contains("IPv6")),
        "訊息說明僅限 IPv6 網段：{}",
        body["message"]
    );

    // 非法位址（v4、非 IPv6 文字、帶前綴）
    for address in ["abc", "10.0.0.1", "fd00::gg", "fd00::1/64"] {
        let (status, body) = register_ip(
            &pool,
            subnet_id,
            json!({ "address": address, "interface_id": interface_id }),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{address} 應被阻擋");
        assert_eq!(body["details"]["field"], "address", "{address}");
    }

    // 不在網段 CIDR 內
    let (status, body) = register_ip(
        &pool,
        subnet_id,
        json!({ "address": "fd01::1", "interface_id": interface_id }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["details"]["field"], "address");
    assert!(
        body["message"]
            .as_str()
            .is_some_and(|message| message.contains("fd00::/64")),
        "訊息含網段：{}",
        body["message"]
    );

    // network 位址可登錄（spec §7：v6 無 host 扣除概念）
    let (status, created) = register_ip(
        &pool,
        subnet_id,
        json!({ "address": "fd00::", "interface_id": interface_id }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "network 位址視為 CIDR 內合法位址：{created}"
    );

    // 缺位址、缺介面、不存在的介面、不存在的網段
    let (status, body) =
        register_ip(&pool, subnet_id, json!({ "interface_id": interface_id })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["details"]["field"], "address");

    let (status, body) = register_ip(&pool, subnet_id, json!({ "address": "fd00::20" })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["details"]["field"], "interface_id");

    let (status, body) = register_ip(
        &pool,
        subnet_id,
        json!({ "address": "fd00::20", "interface_id": 999 }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["details"]["field"], "interface_id");

    let (status, body) = register_ip(
        &pool,
        999,
        json!({ "address": "fd00::20", "interface_id": interface_id }),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"], "not_found");
}

#[tokio::test]
async fn register_enforces_uniqueness_and_cross_subnet_rules() {
    let pool = test_pool().await;
    let first_asset = create_asset(&pool, "設備一", "機房 A").await;
    let second_asset = create_asset(&pool, "設備二", "機房 A").await;
    let first_interface = create_interface(&pool, first_asset, json!({ "name": "eth0" })).await;
    let second_interface = create_interface(&pool, second_asset, json!({ "name": "eth0" })).await;
    let v6_subnet = create_subnet(&pool, json!({ "cidr": "fd00::/64" })).await;
    let other_v6_subnet = create_subnet(&pool, json!({ "cidr": "fd01::/64" })).await;
    let v4_subnet = create_subnet(&pool, json!({ "cidr": "10.0.0.0/29" })).await;

    // 登錄位址給介面一
    let (status, _) = register_ip(
        &pool,
        v6_subnet,
        json!({ "address": "fd00::10", "interface_id": first_interface }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    // 同一 Subnet 同一位址不得重複登錄（含不同壓縮寫法）
    for duplicate in ["fd00::10", "fd00:0:0:0:0:0:0:10"] {
        let (status, body) = register_ip(
            &pool,
            v6_subnet,
            json!({ "address": duplicate, "interface_id": second_interface }),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{duplicate} 應被阻擋");
        assert_eq!(body["details"]["field"], "address");
        assert_eq!(
            body["details"]["interface_id"], first_interface,
            "附目前指派介面"
        );
    }

    // 重複登錄（同一介面）亦回 400：登錄端點不採冪等更新
    let (status, body) = register_ip(
        &pool,
        v6_subnet,
        json!({ "address": "fd00::10", "interface_id": first_interface }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["details"]["field"], "address");

    // 同一介面在同一 Subnet 至多一位址
    let (status, body) = register_ip(
        &pool,
        v6_subnet,
        json!({ "address": "fd00::20", "interface_id": first_interface }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["details"]["field"], "interface_id");
    assert_eq!(body["details"]["existing_address"], "fd00::10");

    // 跨 v6 網段可各一
    let (status, _) = register_ip(
        &pool,
        other_v6_subnet,
        json!({ "address": "fd01::10", "interface_id": first_interface }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "同一介面跨 v6 網段可各有位址");

    // v4+v6 雙棧：同一介面可各有一筆
    let (status, _) = put_assignment(
        &pool,
        v4_subnet,
        "10.0.0.1",
        json!({ "interface_id": first_interface, "purpose": "static" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "同一介面 v4+v6 可並存");
}

#[tokio::test]
async fn put_and_cancel_v6_registry_entry() {
    let pool = test_pool().await;
    let asset_id = create_asset(&pool, "資料庫主機", "機房 A").await;
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
    let subnet_id = create_subnet(&pool, json!({ "cidr": "fd00::/64" })).await;

    let (status, _) = register_ip(
        &pool,
        subnet_id,
        json!({ "address": "fd00::10", "interface_id": first_interface }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    // PUT 同介面同用途：冪等更新
    let (status, updated) = put_assignment(
        &pool,
        subnet_id,
        "fd00::10",
        json!({ "interface_id": first_interface, "purpose": "static" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "重送 static 應成功：{updated}");
    assert_eq!(updated["purpose"], "static");

    // v6 僅允許 static：reservation 回 400
    let (status, body) = put_assignment(
        &pool,
        subnet_id,
        "fd00::10",
        json!({ "interface_id": first_interface, "purpose": "reservation", "hostname": "db-1" }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["details"]["field"], "purpose");
    assert!(
        body["message"]
            .as_str()
            .is_some_and(|message| message.contains("IPv6")),
        "訊息說明 v6 用途固定：{}",
        body["message"]
    );

    // static 不可帶 hostname
    let (status, body) = put_assignment(
        &pool,
        subnet_id,
        "fd00::10",
        json!({ "interface_id": first_interface, "purpose": "static", "hostname": "db-1" }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["details"]["field"], "hostname");

    // 已登錄位址改指派給其他介面：須先取消（ADR-0005）
    let (status, body) = put_assignment(
        &pool,
        subnet_id,
        "fd00::10",
        json!({ "interface_id": second_interface, "purpose": "static" }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["details"]["field"], "address");

    // 未登錄位址可經 PUT 建立（等同指派；前端主流程走 POST）
    let (status, _) = put_assignment(
        &pool,
        subnet_id,
        "fd00::20",
        json!({ "interface_id": second_interface, "purpose": "static" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list_ips(&pool, subnet_id, "").await["total"], 2);

    // 取消指派＝刪除登錄；非壓縮寫法經正規化後刪除
    let (status, body) = delete_assignment(&pool, subnet_id, "fd00:0:0:0:0:0:0:10").await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(body, Value::Null);
    let page = list_ips(&pool, subnet_id, "").await;
    assert_eq!(page["total"], 1);
    assert_eq!(addresses(&page), ["fd00::20"]);

    // 再取消一次：404
    let (status, body) = delete_assignment(&pool, subnet_id, "fd00::10").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"], "not_found");

    // 資產詳情同步更新
    let detail = get_asset(&pool, asset_id).await;
    let assignments = detail["assignments"]
        .as_array()
        .expect("assignments 為陣列");
    assert_eq!(assignments.len(), 1);
    assert_eq!(assignments[0]["address"], "fd00::20");
}

#[tokio::test]
async fn v6_list_supports_search_status_and_pagination() {
    let pool = test_pool().await;
    let first_asset = create_asset(&pool, "資料庫主機", "機房 A").await;
    let second_asset = create_asset(&pool, "印表機", "機房 B").await;
    let first_interface = create_interface(&pool, first_asset, json!({ "name": "eth0" })).await;
    let second_interface = create_interface(
        &pool,
        second_asset,
        json!({ "name": "wlan0", "mac": "AA:BB:CC:DD:EE:FF" }),
    )
    .await;
    let subnet_id = create_subnet(&pool, json!({ "cidr": "fd00::/64" })).await;

    for (address, interface_id) in [("fd00::10", first_interface), ("fd00::2", second_interface)] {
        let (status, _) = register_ip(
            &pool,
            subnet_id,
            json!({ "address": address, "interface_id": interface_id }),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
    }

    // 數值排序：fd00::2 < fd00::10（非文字排序）
    let page = list_ips(&pool, subnet_id, "").await;
    assert_eq!(page["total"], 2);
    assert_eq!(addresses(&page), ["fd00::2", "fd00::10"]);

    // 伺服器端分頁
    let page = list_ips(&pool, subnet_id, "?page=2&per_page=1").await;
    assert_eq!(page["total"], 2);
    assert_eq!(addresses(&page), ["fd00::10"]);

    // 關鍵字：完整位址（含展開寫法）、資產描述、介面名稱、MAC
    for (query, address) in [
        ("fd00:0:0:0:0:0:0:2", "fd00::2"),
        ("資料庫", "fd00::10"),
        ("WLAN0", "fd00::2"),
        ("bb:cc", "fd00::2"),
    ] {
        let page = list_ips(&pool, subnet_id, &format!("?q={query}")).await;
        assert_eq!(page["total"], 1, "q={query}");
        assert_eq!(page["items"][0]["address"], address, "q={query}");
    }

    // 狀態篩選：static 全數；available／in_pool／reservation 皆空
    let page = list_ips(&pool, subnet_id, "?status=static").await;
    assert_eq!(page["total"], 2);
    for status in ["available", "in_pool", "reservation"] {
        let page = list_ips(&pool, subnet_id, &format!("?status={status}")).await;
        assert_eq!(page["total"], 0, "status={status}");
    }
}
