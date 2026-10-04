//! v4 IP 清單整合測試：枚舉邊界、pool 標示、搜尋、排序與分頁（見票 04、14）。

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

/// 新增含廠牌／型號的資產並斷言成功，回傳 id（供票 16 指派對象顯示斷言）。
async fn create_asset_with_brand_model(
    pool: &SqlitePool,
    description: &str,
    location: &str,
    brand: Option<&str>,
    model: Option<&str>,
) -> i64 {
    let (status, json) = send(
        pool,
        Method::POST,
        "/api/v1/assets",
        Some(json!({
            "description": description,
            "location": location,
            "brand": brand,
            "model": model
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

/// 指派位址並斷言成功。
async fn assign_ip(pool: &SqlitePool, subnet_id: i64, address: &str, body: Value) {
    let (status, json) = send(
        pool,
        Method::PUT,
        &format!("/api/v1/subnets/{subnet_id}/ips/{address}/assignment"),
        Some(body),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "指派應成功：{json}");
}

/// v6 登錄位址（新增即指派）並斷言成功。
async fn register_ip(pool: &SqlitePool, subnet_id: i64, address: &str, interface_id: i64) {
    let (status, json) = send(
        pool,
        Method::POST,
        &format!("/api/v1/subnets/{subnet_id}/ips"),
        Some(json!({ "address": address, "interface_id": interface_id })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "登錄位址應成功：{json}");
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

/// 以關鍵字搜尋 IP 清單；`q` 經百分比編碼（容許空白與非 ASCII）。
async fn search_ips(pool: &SqlitePool, id: i64, q: &str) -> Value {
    list_ips(pool, id, &format!("?q={}", encode(q))).await
}

/// 將查詢值編碼為 URI 可接受的百分比格式（僅處理測試用到的字元）。
fn encode(value: &str) -> String {
    value
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                char::from(byte).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect()
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
async fn pool_flags_are_reported() {
    let pool = test_pool().await;

    // pool 有兩段（含單一位址段）；gateway 設定不影響清單列（見票 17）
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

    // gateway 位址非池內、仍為可用（列不再標記，見 spec §7、票 17）
    assert_eq!(item("10.0.0.1")["in_pool"], false);
    assert_eq!(item("10.0.0.1")["status"], "available");

    // pool 內：標示「池內」且無指派用途
    for address in ["10.0.0.2", "10.0.0.3", "10.0.0.6"] {
        assert_eq!(item(address)["in_pool"], true, "{address} 在 pool 內");
        assert_eq!(item(address)["status"], "in_pool", "{address} 狀態為池內");
        assert!(item(address)["purpose"].is_null());
    }

    // pool 外：可用
    assert_eq!(item("10.0.0.4")["in_pool"], false);
    assert_eq!(item("10.0.0.4")["status"], "available");

    // gateway 落在 pool 內：pool 標記不受影響（pool 涵蓋 gateway 提示不擋，見 spec §6）
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
async fn rows_report_assignment_location() {
    let pool = test_pool().await;
    let db_asset = create_asset(&pool, "資料庫主機", "機房 A").await;
    let printer_asset = create_asset(&pool, "印表機", "Server Room B").await;
    let db_interface = create_interface(&pool, db_asset, json!({ "name": "eth0" })).await;
    let printer_interface = create_interface(
        &pool,
        printer_asset,
        json!({ "name": "wlan0", "mac": "aa:bb:cc:dd:ee:ff" }),
    )
    .await;

    // v4：指派列的 `assignment.asset_location` 供「位置」欄顯示；未指派列為 null
    let v4 = create_subnet(&pool, json!({ "cidr": "10.0.0.0/29" })).await;
    let v4_id = v4["id"].as_i64().expect("回應含 id");
    assign_ip(
        &pool,
        v4_id,
        "10.0.0.1",
        json!({ "interface_id": db_interface, "purpose": "static" }),
    )
    .await;
    assign_ip(
        &pool,
        v4_id,
        "10.0.0.2",
        json!({
            "interface_id": printer_interface,
            "purpose": "reservation",
            "hostname": "printer-1"
        }),
    )
    .await;

    let page = list_ips(&pool, v4_id, "").await;
    assert_eq!(
        row(&page, "10.0.0.1")["assignment"]["asset_location"],
        "機房 A"
    );
    assert_eq!(
        row(&page, "10.0.0.2")["assignment"]["asset_location"],
        "Server Room B"
    );
    assert!(
        row(&page, "10.0.0.3")["assignment"].is_null(),
        "未指派列不含位置"
    );

    // 未填廠牌／型號為 null（後端指派對象欄位見票 16）
    assert!(row(&page, "10.0.0.1")["assignment"]["asset_brand"].is_null());
    assert!(row(&page, "10.0.0.1")["assignment"]["asset_model"].is_null());

    // v6：登錄列同樣含位置（v4／v6 回應形狀一致）
    let v6 = create_subnet(&pool, json!({ "cidr": "fd00::/64" })).await;
    let v6_id = v6["id"].as_i64().expect("回應含 id");
    register_ip(&pool, v6_id, "fd00::10", db_interface).await;
    register_ip(&pool, v6_id, "fd00::20", printer_interface).await;

    let page = list_ips(&pool, v6_id, "").await;
    assert_eq!(
        row(&page, "fd00::10")["assignment"]["asset_location"],
        "機房 A"
    );
    assert_eq!(
        row(&page, "fd00::20")["assignment"]["asset_location"],
        "Server Room B"
    );
    assert!(row(&page, "fd00::10")["assignment"]["asset_brand"].is_null());
    assert!(row(&page, "fd00::10")["assignment"]["asset_model"].is_null());
}

#[tokio::test]
async fn rows_report_assignment_asset_brand_and_model() {
    let pool = test_pool().await;

    // 四種缺值組合：廠牌型號皆有、只有廠牌、只有型號、皆無（見票 16）
    let both =
        create_asset_with_brand_model(&pool, "伺服器", "機房 A", Some("Dell"), Some("R740")).await;
    let brand_only =
        create_asset_with_brand_model(&pool, "交換器", "機房 B", Some("Cisco"), None).await;
    let model_only =
        create_asset_with_brand_model(&pool, "印表機", "機房 C", None, Some("LaserJet")).await;
    let neither = create_asset(&pool, "測試機", "機房 D").await;

    let both_interface = create_interface(&pool, both, json!({ "name": "eth0" })).await;
    let brand_interface = create_interface(&pool, brand_only, json!({ "name": "eth1" })).await;
    let model_interface = create_interface(&pool, model_only, json!({ "name": "eth2" })).await;
    let neither_interface = create_interface(&pool, neither, json!({ "name": "eth3" })).await;

    // v4：指派列的 `assignment` 帶出廠牌／型號（IP 清單第一行組「描述(廠牌 型號)」用）
    let v4 = create_subnet(&pool, json!({ "cidr": "10.0.0.0/29" })).await;
    let v4_id = v4["id"].as_i64().expect("回應含 id");
    for (address, interface_id) in [
        ("10.0.0.1", both_interface),
        ("10.0.0.2", brand_interface),
        ("10.0.0.3", model_interface),
        ("10.0.0.4", neither_interface),
    ] {
        assign_ip(
            &pool,
            v4_id,
            address,
            json!({ "interface_id": interface_id, "purpose": "static" }),
        )
        .await;
    }

    let page = list_ips(&pool, v4_id, "").await;
    assert_eq!(row(&page, "10.0.0.1")["assignment"]["asset_brand"], "Dell");
    assert_eq!(row(&page, "10.0.0.1")["assignment"]["asset_model"], "R740");
    assert_eq!(row(&page, "10.0.0.2")["assignment"]["asset_brand"], "Cisco");
    assert!(
        row(&page, "10.0.0.2")["assignment"]["asset_model"].is_null(),
        "只有廠牌：型號為 null"
    );
    assert!(
        row(&page, "10.0.0.3")["assignment"]["asset_brand"].is_null(),
        "只有型號：廠牌為 null"
    );
    assert_eq!(
        row(&page, "10.0.0.3")["assignment"]["asset_model"],
        "LaserJet"
    );
    assert!(
        row(&page, "10.0.0.4")["assignment"]["asset_brand"].is_null(),
        "皆無：廠牌為 null"
    );
    assert!(
        row(&page, "10.0.0.4")["assignment"]["asset_model"].is_null(),
        "皆無：型號為 null"
    );

    // v6：登錄列回應形狀與 v4 一致（前端 v4／v6 共用同一 cell）
    let v6 = create_subnet(&pool, json!({ "cidr": "fd00::/64" })).await;
    let v6_id = v6["id"].as_i64().expect("回應含 id");
    register_ip(&pool, v6_id, "fd00::1", both_interface).await;
    register_ip(&pool, v6_id, "fd00::2", brand_interface).await;
    register_ip(&pool, v6_id, "fd00::3", model_interface).await;
    register_ip(&pool, v6_id, "fd00::4", neither_interface).await;

    let page = list_ips(&pool, v6_id, "").await;
    assert_eq!(row(&page, "fd00::1")["assignment"]["asset_brand"], "Dell");
    assert_eq!(row(&page, "fd00::1")["assignment"]["asset_model"], "R740");
    assert_eq!(row(&page, "fd00::2")["assignment"]["asset_brand"], "Cisco");
    assert!(row(&page, "fd00::2")["assignment"]["asset_model"].is_null());
    assert!(row(&page, "fd00::3")["assignment"]["asset_brand"].is_null());
    assert_eq!(
        row(&page, "fd00::3")["assignment"]["asset_model"],
        "LaserJet"
    );
    assert!(row(&page, "fd00::4")["assignment"]["asset_brand"].is_null());
    assert!(row(&page, "fd00::4")["assignment"]["asset_model"].is_null());
}

#[tokio::test]
async fn search_matches_assignment_location() {
    let pool = test_pool().await;
    let db_asset = create_asset(&pool, "資料庫主機", "機房 A").await;
    let printer_asset = create_asset(&pool, "印表機", "Server Room B").await;
    let db_interface = create_interface(&pool, db_asset, json!({ "name": "eth0" })).await;
    let printer_interface = create_interface(
        &pool,
        printer_asset,
        json!({ "name": "wlan0", "mac": "aa:bb:cc:dd:ee:ff" }),
    )
    .await;

    let v4 = create_subnet(&pool, json!({ "cidr": "10.0.0.0/29" })).await;
    let v4_id = v4["id"].as_i64().expect("回應含 id");
    assign_ip(
        &pool,
        v4_id,
        "10.0.0.1",
        json!({ "interface_id": db_interface, "purpose": "static" }),
    )
    .await;
    assign_ip(
        &pool,
        v4_id,
        "10.0.0.2",
        json!({
            "interface_id": printer_interface,
            "purpose": "reservation",
            "hostname": "printer-1"
        }),
    )
    .await;

    // 位置關鍵字：子字串比對、不分大小寫；僅命中已指派列
    let page = search_ips(&pool, v4_id, "機房").await;
    assert_eq!(page["total"], 1);
    assert_eq!(addresses(&page), ["10.0.0.1"]);

    let page = search_ips(&pool, v4_id, "server room").await;
    assert_eq!(page["total"], 1);
    assert_eq!(addresses(&page), ["10.0.0.2"]);

    // 與狀態篩選並用（AND）：位置的用途為 static，reservation 應排除
    let page = list_ips(
        &pool,
        v4_id,
        &format!("?q={}&status=static", encode("機房")),
    )
    .await;
    assert_eq!(page["total"], 1);
    assert_eq!(addresses(&page), ["10.0.0.1"]);

    let page = list_ips(
        &pool,
        v4_id,
        &format!("?q={}&status=reservation", encode("機房")),
    )
    .await;
    assert_eq!(page["total"], 0);

    // 同一關鍵字可比對位置或既有欄位（描述）並累計結果
    let page = search_ips(&pool, v4_id, "機").await;
    assert_eq!(page["total"], 2);
    assert_eq!(addresses(&page), ["10.0.0.1", "10.0.0.2"]);

    // 無結果
    let page = search_ips(&pool, v4_id, "不存在的機房").await;
    assert_eq!(page["total"], 0);
    assert!(addresses(&page).is_empty());

    // v6：登錄清單的位置搜尋行為一致
    let v6 = create_subnet(&pool, json!({ "cidr": "fd00::/64" })).await;
    let v6_id = v6["id"].as_i64().expect("回應含 id");
    register_ip(&pool, v6_id, "fd00::10", db_interface).await;
    register_ip(&pool, v6_id, "fd00::20", printer_interface).await;

    let page = search_ips(&pool, v6_id, "機房").await;
    assert_eq!(page["total"], 1);
    assert_eq!(addresses(&page), ["fd00::10"]);

    let page = search_ips(&pool, v6_id, "SERVER ROOM").await;
    assert_eq!(page["total"], 1);
    assert_eq!(addresses(&page), ["fd00::20"]);

    let page = search_ips(&pool, v6_id, "不存在").await;
    assert_eq!(page["total"], 0);
    assert!(addresses(&page).is_empty());
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

// ---- 標頭排序（見票 14）----

#[tokio::test]
async fn list_sorts_by_whitelisted_columns_with_direction() {
    let pool = test_pool().await;

    // /29、pool .5–.6；.2 static（alpha／server room）、
    // .3 保留（Zeta／Server Room，與 .2 位置同鍵）、.4 可用
    let subnet = create_subnet(
        &pool,
        json!({
            "cidr": "10.0.0.0/29",
            "pools": [{ "start_ip": "10.0.0.5", "end_ip": "10.0.0.6" }]
        }),
    )
    .await;
    let id = subnet["id"].as_i64().expect("回應含 id");

    let alpha_asset = create_asset(&pool, "alpha", "server room").await;
    let zeta_asset = create_asset(&pool, "Zeta", "Server Room").await;
    let alpha_interface = create_interface(&pool, alpha_asset, json!({ "name": "eth0" })).await;
    let zeta_interface = create_interface(
        &pool,
        zeta_asset,
        json!({ "name": "eth1", "mac": "aa:bb:cc:dd:ee:ff" }),
    )
    .await;

    assign_ip(
        &pool,
        id,
        "10.0.0.2",
        json!({ "interface_id": alpha_interface, "purpose": "static" }),
    )
    .await;
    assign_ip(
        &pool,
        id,
        "10.0.0.3",
        json!({
            "interface_id": zeta_interface,
            "purpose": "reservation",
            "hostname": "zeta-1"
        }),
    )
    .await;

    // 預設（未帶排序參數）與顯式 address 升冪完全相同
    let default_page = list_ips(&pool, id, "").await;
    let explicit = list_ips(&pool, id, "?sort=address&dir=asc").await;
    assert_eq!(
        addresses(&default_page),
        addresses(&explicit),
        "預設即 address 升冪"
    );

    let cases = [
        (
            "sort=address&dir=desc",
            vec![
                "10.0.0.6", "10.0.0.5", "10.0.0.4", "10.0.0.3", "10.0.0.2", "10.0.0.1",
            ],
        ),
        // 狀態 asc：可用→池內→手動設定→保留；desc 反轉（同鍵位址升冪）
        (
            "sort=status&dir=asc",
            vec![
                "10.0.0.1", "10.0.0.4", "10.0.0.5", "10.0.0.6", "10.0.0.2", "10.0.0.3",
            ],
        ),
        (
            "sort=status&dir=desc",
            vec![
                "10.0.0.3", "10.0.0.2", "10.0.0.5", "10.0.0.6", "10.0.0.1", "10.0.0.4",
            ],
        ),
        // 位置：.2／.3 同鍵（不分大小寫）以位址升冪；未指派（.1、.4–.6）asc、desc 皆最後
        (
            "sort=location&dir=asc",
            vec![
                "10.0.0.2", "10.0.0.3", "10.0.0.1", "10.0.0.4", "10.0.0.5", "10.0.0.6",
            ],
        ),
        (
            "sort=location&dir=desc",
            vec![
                "10.0.0.2", "10.0.0.3", "10.0.0.1", "10.0.0.4", "10.0.0.5", "10.0.0.6",
            ],
        ),
        // 指派對象 asc：alpha < Zeta（不分大小寫）；未指派最後
        (
            "sort=assignment&dir=asc",
            vec![
                "10.0.0.2", "10.0.0.3", "10.0.0.1", "10.0.0.4", "10.0.0.5", "10.0.0.6",
            ],
        ),
        (
            "sort=assignment&dir=desc",
            vec![
                "10.0.0.3", "10.0.0.2", "10.0.0.1", "10.0.0.4", "10.0.0.5", "10.0.0.6",
            ],
        ),
    ];
    for (query, expected) in cases {
        let page = list_ips(&pool, id, &format!("?{query}")).await;
        assert_eq!(addresses(&page), expected, "{query}");
    }
}

#[tokio::test]
async fn location_and_assignment_sorts_are_case_insensitive_with_blanks_last() {
    let pool = test_pool().await;
    let subnet = create_subnet(&pool, json!({ "cidr": "10.0.0.0/29" })).await;
    let id = subnet["id"].as_i64().expect("回應含 id");

    // 位置與指派對象皆含大小寫差異；.1／.5／.6 未指派
    let b_asset = create_asset(&pool, "alpha", "機房 B").await;
    let a_asset = create_asset(&pool, "Bravo", "機房 a").await;
    let s_asset = create_asset(&pool, "beta", "server").await;
    let b_interface = create_interface(&pool, b_asset, json!({ "name": "eth0" })).await;
    let a_interface = create_interface(&pool, a_asset, json!({ "name": "eth1" })).await;
    let s_interface = create_interface(&pool, s_asset, json!({ "name": "eth2" })).await;

    assign_ip(
        &pool,
        id,
        "10.0.0.2",
        json!({ "interface_id": b_interface, "purpose": "static" }),
    )
    .await;
    assign_ip(
        &pool,
        id,
        "10.0.0.3",
        json!({ "interface_id": a_interface, "purpose": "static" }),
    )
    .await;
    assign_ip(
        &pool,
        id,
        "10.0.0.4",
        json!({ "interface_id": s_interface, "purpose": "static" }),
    )
    .await;

    // 位置 asc：server < 機房 a < 機房 B（不分大小寫）→ .4、.3、.2；未指派最後
    let page = list_ips(&pool, id, "?sort=location&dir=asc").await;
    assert_eq!(
        addresses(&page),
        [
            "10.0.0.4", "10.0.0.3", "10.0.0.2", "10.0.0.1", "10.0.0.5", "10.0.0.6"
        ]
    );

    // 位置 desc：反轉已指派組；未指派仍固定最後
    let page = list_ips(&pool, id, "?sort=location&dir=desc").await;
    assert_eq!(
        addresses(&page),
        [
            "10.0.0.2", "10.0.0.3", "10.0.0.4", "10.0.0.1", "10.0.0.5", "10.0.0.6"
        ]
    );

    // 指派對象 asc：alpha < beta < Bravo（不分大小寫）
    let page = list_ips(&pool, id, "?sort=assignment&dir=asc").await;
    assert_eq!(
        addresses(&page),
        [
            "10.0.0.2", "10.0.0.4", "10.0.0.3", "10.0.0.1", "10.0.0.5", "10.0.0.6"
        ]
    );

    // 指派對象 desc：反轉；未指派仍固定最後
    let page = list_ips(&pool, id, "?sort=assignment&dir=desc").await;
    assert_eq!(
        addresses(&page),
        [
            "10.0.0.3", "10.0.0.4", "10.0.0.2", "10.0.0.1", "10.0.0.5", "10.0.0.6"
        ]
    );
}

#[tokio::test]
async fn sorts_combine_with_q_and_status_filters() {
    let pool = test_pool().await;
    let subnet = create_subnet(&pool, json!({ "cidr": "10.0.0.0/29" })).await;
    let id = subnet["id"].as_i64().expect("回應含 id");

    let b_asset = create_asset(&pool, "alpha", "機房 B").await;
    let a_asset = create_asset(&pool, "Bravo", "機房 a").await;
    let s_asset = create_asset(&pool, "beta", "server").await;
    let b_interface = create_interface(&pool, b_asset, json!({ "name": "eth0" })).await;
    let a_interface = create_interface(&pool, a_asset, json!({ "name": "eth1" })).await;
    let s_interface = create_interface(&pool, s_asset, json!({ "name": "eth2" })).await;

    assign_ip(
        &pool,
        id,
        "10.0.0.2",
        json!({ "interface_id": b_interface, "purpose": "static" }),
    )
    .await;
    assign_ip(
        &pool,
        id,
        "10.0.0.3",
        json!({ "interface_id": a_interface, "purpose": "static" }),
    )
    .await;
    assign_ip(
        &pool,
        id,
        "10.0.0.4",
        json!({ "interface_id": s_interface, "purpose": "static" }),
    )
    .await;

    // q（位置）＋指派對象 desc：僅命中「機房」的 .2、.3；Bravo > alpha
    let page = list_ips(
        &pool,
        id,
        &format!("?q={}&sort=assignment&dir=desc", encode("機房")),
    )
    .await;
    assert_eq!(page["total"], 2);
    assert_eq!(addresses(&page), ["10.0.0.3", "10.0.0.2"]);

    // 狀態＋位置 asc：三列皆 static；server(.4) < 機房 a(.3) < 機房 B(.2)
    let page = list_ips(&pool, id, "?status=static&sort=location&dir=asc").await;
    assert_eq!(page["total"], 3);
    assert_eq!(addresses(&page), ["10.0.0.4", "10.0.0.3", "10.0.0.2"]);

    // 三者並用：q＋status＋排序
    let page = list_ips(
        &pool,
        id,
        &format!(
            "?q={}&status=static&sort=assignment&dir=asc",
            encode("機房")
        ),
    )
    .await;
    assert_eq!(page["total"], 2);
    assert_eq!(addresses(&page), ["10.0.0.2", "10.0.0.3"]);

    // 無結果的組合維持空集合
    let page = list_ips(
        &pool,
        id,
        &format!("?q={}&sort=assignment&dir=desc", encode("不存在")),
    )
    .await;
    assert_eq!(page["total"], 0);
    assert!(addresses(&page).is_empty());
}

#[tokio::test]
async fn v6_list_sorts_consistently() {
    let pool = test_pool().await;
    let subnet = create_subnet(&pool, json!({ "cidr": "fd00::/64" })).await;
    let id = subnet["id"].as_i64().expect("回應含 id");

    let zeta_asset = create_asset(&pool, "Zeta", "機房 B").await;
    let alpha_asset = create_asset(&pool, "alpha", "機房 a").await;
    let zeta_interface = create_interface(&pool, zeta_asset, json!({ "name": "eth0" })).await;
    let alpha_interface = create_interface(&pool, alpha_asset, json!({ "name": "eth1" })).await;

    register_ip(&pool, id, "fd00::10", zeta_interface).await;
    register_ip(&pool, id, "fd00::2", alpha_interface).await;

    // 預設：位址數值升冪
    let page = list_ips(&pool, id, "").await;
    assert_eq!(addresses(&page), ["fd00::2", "fd00::10"]);

    // 位址 desc：數值反序（非文字序）
    let page = list_ips(&pool, id, "?sort=address&dir=desc").await;
    assert_eq!(addresses(&page), ["fd00::10", "fd00::2"]);

    // 位置 asc：機房 a < 機房 B（不分大小寫）
    let page = list_ips(&pool, id, "?sort=location&dir=asc").await;
    assert_eq!(addresses(&page), ["fd00::2", "fd00::10"]);

    let page = list_ips(&pool, id, "?sort=location&dir=desc").await;
    assert_eq!(addresses(&page), ["fd00::10", "fd00::2"]);

    // 指派對象 desc：Zeta > alpha
    let page = list_ips(&pool, id, "?sort=assignment&dir=desc").await;
    assert_eq!(addresses(&page), ["fd00::10", "fd00::2"]);

    // 與狀態篩選組合（v6 恆 static；available 篩選為空）
    let page = list_ips(&pool, id, "?status=static&sort=assignment&dir=asc").await;
    assert_eq!(addresses(&page), ["fd00::2", "fd00::10"]);

    let page = list_ips(&pool, id, "?status=available&sort=assignment&dir=asc").await;
    assert_eq!(page["total"], 0);
    assert!(addresses(&page).is_empty());
}

#[tokio::test]
async fn non_default_sort_includes_out_of_subnet_assignments() {
    let pool = test_pool().await;
    // 先以 /24 指派，再縮小為 /25：.200 成為出界指派列（不阻擋，見票 07）
    let subnet = create_subnet(&pool, json!({ "cidr": "10.0.0.0/24" })).await;
    let id = subnet["id"].as_i64().expect("回應含 id");

    let out_asset = create_asset(&pool, "出界主機", "機房 A").await;
    let in_asset = create_asset(&pool, "本段主機", "機房 B").await;
    let out_interface = create_interface(&pool, out_asset, json!({ "name": "eth0" })).await;
    let in_interface = create_interface(&pool, in_asset, json!({ "name": "eth1" })).await;

    assign_ip(
        &pool,
        id,
        "10.0.0.200",
        json!({ "interface_id": out_interface, "purpose": "static" }),
    )
    .await;
    assign_ip(
        &pool,
        id,
        "10.0.0.2",
        json!({ "interface_id": in_interface, "purpose": "static" }),
    )
    .await;

    let (status, updated) = send(
        &pool,
        Method::PATCH,
        &format!("/api/v1/subnets/{id}"),
        Some(json!({ "cidr": "10.0.0.0/25" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "縮小 CIDR 不阻擋：{updated}");

    // 位址 desc：出界列數值最大，排第一
    let page = list_ips(&pool, id, "?sort=address&dir=desc").await;
    assert_eq!(page["total"], 127);
    assert_eq!(addresses(&page)[0], "10.0.0.200");

    // 位置 asc：機房 A（.200）先於機房 B（.2）；未指派列最後
    let page = list_ips(&pool, id, "?sort=location&dir=asc").await;
    assert_eq!(page["total"], 127);
    assert_eq!(addresses(&page)[..2].to_vec(), ["10.0.0.200", "10.0.0.2"]);

    // 位置 desc 與 q 組合：命中「機房」的兩列反序
    let page = list_ips(
        &pool,
        id,
        &format!("?q={}&sort=location&dir=desc", encode("機房")),
    )
    .await;
    assert_eq!(page["total"], 2);
    assert_eq!(addresses(&page), ["10.0.0.2", "10.0.0.200"]);
}

#[tokio::test]
async fn invalid_sort_and_dir_return_400() {
    let pool = test_pool().await;
    let subnet = create_subnet(&pool, json!({ "cidr": "10.0.0.0/29" })).await;
    let id = subnet["id"].as_i64().expect("回應含 id");

    let (status, body) = send(
        &pool,
        Method::GET,
        &format!("/api/v1/subnets/{id}/ips?sort=id"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "validation_error");
    assert_eq!(body["details"]["field"], "sort");
    assert!(
        body["message"].as_str().expect("訊息為字串").contains("id"),
        "錯誤訊息應指出無效值"
    );

    // 衝突、操作不可排序
    let (status, body) = send(
        &pool,
        Method::GET,
        &format!("/api/v1/subnets/{id}/ips?sort=conflicts"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["details"]["field"], "sort");

    // gateway 已移除排序白名單（見票 17）
    let (status, body) = send(
        &pool,
        Method::GET,
        &format!("/api/v1/subnets/{id}/ips?sort=gateway"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "validation_error");
    assert_eq!(body["details"]["field"], "sort");
    assert!(
        body["message"]
            .as_str()
            .expect("訊息為字串")
            .contains("gateway"),
        "錯誤訊息應指出無效值"
    );

    let (status, body) = send(
        &pool,
        Method::GET,
        &format!("/api/v1/subnets/{id}/ips?sort=address&dir=sideways"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "validation_error");
    assert_eq!(body["details"]["field"], "dir");

    // 有效值不受影響
    let (status, _) = send(
        &pool,
        Method::GET,
        &format!("/api/v1/subnets/{id}/ips?sort=location&dir=desc"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn default_sort_stays_numeric_ascending_and_other_sorts_paginate() {
    let pool = test_pool().await;
    let subnet = create_subnet(&pool, json!({ "cidr": "10.0.0.0/27" })).await;
    let id = subnet["id"].as_i64().expect("回應含 id");

    // 未帶排序參數：數值升冪（與既有行為相同）
    let page = list_ips(&pool, id, "").await;
    assert_eq!(page["total"], 30);
    assert_eq!(
        addresses(&page)[..3].to_vec(),
        ["10.0.0.1", "10.0.0.2", "10.0.0.3"]
    );

    // 非預設排序＋伺服器端分頁：當頁取自排序後結果
    let page = list_ips(&pool, id, "?sort=address&dir=desc&page=2&per_page=10").await;
    assert_eq!(page["total"], 30);
    assert_eq!(page["page"], 2);
    assert_eq!(page["per_page"], 10);
    assert_eq!(
        addresses(&page),
        [
            "10.0.0.20",
            "10.0.0.19",
            "10.0.0.18",
            "10.0.0.17",
            "10.0.0.16",
            "10.0.0.15",
            "10.0.0.14",
            "10.0.0.13",
            "10.0.0.12",
            "10.0.0.11"
        ]
    );

    // 超出範圍的頁：空列但總數不變
    let page = list_ips(&pool, id, "?sort=status&dir=asc&page=9&per_page=10").await;
    assert_eq!(page["total"], 30);
    assert!(addresses(&page).is_empty());
}
