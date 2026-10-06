//! 指派候選整合測試：前綴語意、可用性過濾、limit、`query_status` 與 400 邊界
//! （見 spec §8、票 04）。

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

/// 指派／改用途並斷言成功。
async fn put_assignment(pool: &SqlitePool, subnet_id: i64, address: &str, body: Value) {
    let (status, json) = send(
        pool,
        Method::PUT,
        &format!("/api/v1/subnets/{subnet_id}/ips/{address}/assignment"),
        Some(body),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "指派 {address} 應成功：{json}");
}

/// 查詢候選（不檢查狀態碼）；`query` 為含 `?` 的查詢字串。
async fn get_candidates(pool: &SqlitePool, query: &str) -> (StatusCode, Value) {
    send(
        pool,
        Method::GET,
        &format!("/api/v1/ip-candidates{query}"),
        None,
    )
    .await
}

/// 查詢候選並斷言成功。
async fn candidates_ok(pool: &SqlitePool, query: &str) -> Value {
    let (status, json) = get_candidates(pool, query).await;
    assert_eq!(status, StatusCode::OK, "查詢候選應成功：{json}");
    json
}

/// 取出 `items` 的位址清單。
fn addresses(page: &Value) -> Vec<String> {
    page["items"]
        .as_array()
        .expect("items 為陣列")
        .iter()
        .map(|item| {
            item["address"]
                .as_str()
                .expect("address 為字串")
                .to_string()
        })
        .collect()
}

/// 取出 `items` 中某位址的候選列。
fn candidate<'a>(page: &'a Value, address: &str) -> &'a Value {
    page["items"]
        .as_array()
        .expect("items 為陣列")
        .iter()
        .find(|item| item["address"] == address)
        .unwrap_or_else(|| panic!("候選含 {address}"))
}

/// 讀取單一完整位址的 `query_status`。
async fn query_status(pool: &SqlitePool, q: &str) -> Value {
    candidates_ok(pool, &format!("?q={q}")).await["query_status"].clone()
}

#[tokio::test]
async fn prefix_semantics_stay_within_octet_boundaries() {
    let pool = test_pool().await;
    let first_id = create_subnet(&pool, json!({ "cidr": "10.0.1.0/28", "name": "一區" })).await;
    let second_id = create_subnet(&pool, json!({ "cidr": "10.0.10.0/28", "name": "十區" })).await;
    create_subnet(&pool, json!({ "cidr": "10.0.2.0/28", "name": "二區" })).await;

    // 結尾帶點：第三段精確為 1 → 只含 10.0.1.x（不含 10.0.10.5）。
    let page = candidates_ok(&pool, "?q=10.0.1.").await;
    let found = addresses(&page);
    assert_eq!(found.len(), 14, "10.0.1.0/28 的 14 個 host：{found:?}");
    assert_eq!(found.first().map(String::as_str), Some("10.0.1.1"));
    assert_eq!(found.last().map(String::as_str), Some("10.0.1.14"));
    assert!(found.iter().all(|address| address.starts_with("10.0.1.")));
    assert!(!found.iter().any(|address| address == "10.0.10.5"));
    assert!(page["query_status"].is_null(), "非完整位址無狀態");
    assert!(
        !page.as_object().expect("物件").contains_key("total"),
        "回應不含 total"
    );

    let item = candidate(&page, "10.0.1.1");
    assert_eq!(item["subnet_id"], first_id);
    assert_eq!(item["subnet_cidr"], "10.0.1.0/28");
    assert_eq!(item["subnet_name"], "一區");

    // 未打點：最後一段為前綴段 → 第三段 ∈ {1, 10–19, 100–199}，
    // 同時含 10.0.1.x 與 10.0.10.x；不含第三段 2／9／20 的位址。
    let page = candidates_ok(&pool, "?q=10.0.1&limit=50").await;
    let found = addresses(&page);
    assert_eq!(found.len(), 28, "兩段 /28 共 28 個 host：{found:?}");
    assert!(found.contains(&"10.0.1.1".to_string()));
    assert!(found.contains(&"10.0.10.1".to_string()));
    assert!(!found.iter().any(|address| address.starts_with("10.0.2.")));
    assert!(!found.iter().any(|address| address.starts_with("10.0.9.")));
    assert!(!found.iter().any(|address| address.starts_with("10.0.20.")));
    assert_eq!(
        (found[0].as_str(), found[1].as_str()),
        ("10.0.1.1", "10.0.10.1"),
        "前綴段值輪流取樣：兩網段各先取一筆"
    );
    assert!(page["query_status"].is_null(), "前綴段非完整位址");
    assert_eq!(candidate(&page, "10.0.10.1")["subnet_id"], second_id);

    // 值輪流取樣：limit 小時仍涵蓋多個前綴段值（兩網段各兩筆）。
    let page = candidates_ok(&pool, "?q=10.0.1&limit=4").await;
    assert_eq!(
        addresses(&page),
        ["10.0.1.1", "10.0.10.1", "10.0.1.2", "10.0.10.2"]
    );

    // 無所屬網段 → 空陣列。
    let page = candidates_ok(&pool, "?q=172.16.1.").await;
    assert!(addresses(&page).is_empty());
    assert!(page["query_status"].is_null());
}

#[tokio::test]
async fn complete_address_query_expands_last_octet() {
    let pool = test_pool().await;
    let subnet_id = create_subnet(
        &pool,
        json!({ "cidr": "10.0.9.0/24", "name": "完整位址區" }),
    )
    .await;

    // 10.0.9.6（無結尾點）＝前三段完整、第四段前綴段 {6, 60–69}；
    // query_status 仍為完整位址的狀態。
    let page = candidates_ok(&pool, "?q=10.0.9.6&limit=50").await;
    assert_eq!(
        addresses(&page),
        [
            "10.0.9.6",
            "10.0.9.60",
            "10.0.9.61",
            "10.0.9.62",
            "10.0.9.63",
            "10.0.9.64",
            "10.0.9.65",
            "10.0.9.66",
            "10.0.9.67",
            "10.0.9.68",
            "10.0.9.69"
        ]
    );
    assert_eq!(page["query_status"]["address"], "10.0.9.6");
    assert_eq!(page["query_status"]["status"], "available");
    assert_eq!(candidate(&page, "10.0.9.60")["subnet_id"], subnet_id);

    // 結尾帶點：完整位址精確單一筆。
    let page = candidates_ok(&pool, "?q=10.0.9.6.").await;
    assert_eq!(addresses(&page), ["10.0.9.6"]);
    assert_eq!(page["query_status"]["status"], "available");
}

#[tokio::test]
async fn items_only_include_available_hosts() {
    let pool = test_pool().await;
    let asset_id = create_asset(&pool, "測試主機", "機房 A").await;
    let interface_id = create_interface(&pool, asset_id, json!({ "name": "eth0" })).await;
    let subnet_id = create_subnet(
        &pool,
        json!({
            "cidr": "10.0.1.0/28",
            "name": "過濾區",
            "pools": [{ "start_ip": "10.0.1.2", "end_ip": "10.0.1.3" }],
            "exclusions": [{ "start_ip": "10.0.1.6", "end_ip": "10.0.1.7" }]
        }),
    )
    .await;

    put_assignment(
        &pool,
        subnet_id,
        "10.0.1.4",
        json!({ "interface_id": interface_id, "purpose": "static" }),
    )
    .await;

    let page = candidates_ok(&pool, "?q=10.0.1.&limit=50").await;
    assert_eq!(
        addresses(&page),
        [
            "10.0.1.1",
            "10.0.1.5",
            "10.0.1.8",
            "10.0.1.9",
            "10.0.1.10",
            "10.0.1.11",
            "10.0.1.12",
            "10.0.1.13",
            "10.0.1.14"
        ],
        "排除 network／broadcast、pool、排除範圍與已指派"
    );

    // 完整位址（無尾點）＝精確查詢：只回該位址，且 query_status 為可用。
    let page = candidates_ok(&pool, "?q=10.0.1.5").await;
    assert_eq!(addresses(&page), ["10.0.1.5"]);
    assert_eq!(page["query_status"]["status"], "available");

    // 完整但已指派 → items 不含；query_status 回報指派用途。
    let page = candidates_ok(&pool, "?q=10.0.1.4").await;
    assert!(addresses(&page).is_empty());
    assert_eq!(page["query_status"]["status"], "static");
}

#[tokio::test]
async fn limit_defaults_and_clamps() {
    let pool = test_pool().await;
    create_subnet(&pool, json!({ "cidr": "10.9.0.0/24" })).await;
    create_subnet(&pool, json!({ "cidr": "fd00::/64" })).await;

    // 預設 20。
    let page = candidates_ok(&pool, "?q=10.9.0.").await;
    let found = addresses(&page);
    assert_eq!(found.len(), 20);
    assert_eq!(found.first().map(String::as_str), Some("10.9.0.1"));
    assert_eq!(found.last().map(String::as_str), Some("10.9.0.20"));

    // 指定 limit。
    let page = candidates_ok(&pool, "?q=10.9.0.&limit=5").await;
    assert_eq!(addresses(&page).len(), 5);

    // 上限 50：>50 夾住。
    let page = candidates_ok(&pool, "?q=10.9.0.&limit=50").await;
    assert_eq!(addresses(&page).len(), 50);
    let page = candidates_ok(&pool, "?q=10.9.0.&limit=100").await;
    assert_eq!(addresses(&page).len(), 50, "超過上限夾在 50");

    // 下限 1：0／負數夾住。
    let page = candidates_ok(&pool, "?q=10.9.0.&limit=0").await;
    assert_eq!(addresses(&page).len(), 1);
    let page = candidates_ok(&pool, "?q=10.9.0.&limit=-3").await;
    assert_eq!(addresses(&page).len(), 1);
}

#[tokio::test]
async fn query_status_reports_all_states_in_priority_order() {
    let pool = test_pool().await;
    let asset_id = create_asset(&pool, "測試主機", "機房 A").await;
    let static_interface = create_interface(&pool, asset_id, json!({ "name": "eth0" })).await;
    let reservation_interface = create_interface(
        &pool,
        asset_id,
        json!({ "name": "eth1", "mac": "AA:BB:CC:DD:EE:01" }),
    )
    .await;
    let subnet_id = create_subnet(
        &pool,
        json!({
            "cidr": "10.0.2.0/28",
            "name": "狀態區",
            "pools": [{ "start_ip": "10.0.2.2", "end_ip": "10.0.2.2" }],
            "exclusions": [{ "start_ip": "10.0.2.6", "end_ip": "10.0.2.6" }]
        }),
    )
    .await;

    put_assignment(
        &pool,
        subnet_id,
        "10.0.2.3",
        json!({ "interface_id": static_interface, "purpose": "static" }),
    )
    .await;
    put_assignment(
        &pool,
        subnet_id,
        "10.0.2.4",
        json!({
            "interface_id": reservation_interface,
            "purpose": "reservation",
            "hostname": "printer-1"
        }),
    )
    .await;

    // available：落在網段、未指派、非池內、非排除範圍。
    let status = query_status(&pool, "10.0.2.1").await;
    assert_eq!(status["status"], "available");
    assert_eq!(status["address"], "10.0.2.1");
    assert_eq!(status["subnet_id"], subnet_id);
    assert_eq!(status["subnet_cidr"], "10.0.2.0/28");
    assert_eq!(status["subnet_name"], "狀態區");

    // in_pool／excluded。
    assert_eq!(query_status(&pool, "10.0.2.2").await["status"], "in_pool");
    assert_eq!(query_status(&pool, "10.0.2.6").await["status"], "excluded");

    // static／reservation：有指派以用途為準。
    assert_eq!(query_status(&pool, "10.0.2.3").await["status"], "static");
    assert_eq!(
        query_status(&pool, "10.0.2.4").await["status"],
        "reservation"
    );

    // 非 host（network／broadcast）→ out_of_subnet，但仍在網段 CIDR 內。
    for address in ["10.0.2.0", "10.0.2.15"] {
        let status = query_status(&pool, address).await;
        assert_eq!(status["status"], "out_of_subnet", "{address}");
        assert_eq!(status["subnet_id"], subnet_id, "{address}");
        assert_eq!(status["subnet_cidr"], "10.0.2.0/28", "{address}");
    }

    // 完全無所屬網段 → out_of_subnet、subnet 欄位 null。
    let status = query_status(&pool, "192.168.1.1").await;
    assert_eq!(status["status"], "out_of_subnet");
    assert_eq!(status["address"], "192.168.1.1");
    assert!(status["subnet_id"].is_null());
    assert!(status["subnet_cidr"].is_null());
    assert!(status["subnet_name"].is_null());
}

#[tokio::test]
async fn invalid_prefix_returns_400_with_field_q() {
    let pool = test_pool().await;

    // q 缺：0 個完整 octet。
    let (status, body) = get_candidates(&pool, "").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "validation_error");
    assert_eq!(body["details"]["field"], "q");
    assert_eq!(body["details"]["reason"], "invalid_query");
    assert_eq!(
        body["message"],
        "查詢前綴至少須包含兩個完整 octet（例：10.0.）"
    );

    // 只有 1 個完整 octet；顯式空字串亦同。
    for query in ["?q=10", "?q="] {
        let (status, body) = get_candidates(&pool, query).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{query}");
        assert_eq!(body["details"]["field"], "q", "{query}");
        assert_eq!(
            body["message"], "查詢前綴至少須包含兩個完整 octet（例：10.0.）",
            "{query}"
        );
    }

    // 超過 4 段、非數字、完整 octet > 255、空段。
    for q in ["10.0.1.2.3", "abc", "10.256.0", "10..0", ".10.0"] {
        let (status, body) = get_candidates(&pool, &format!("?q={q}")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "q={q}");
        assert_eq!(body["error"], "validation_error", "q={q}");
        assert_eq!(body["details"]["field"], "q", "q={q}");
        assert_eq!(body["details"]["reason"], "invalid_query", "q={q}");
        assert!(
            body["message"]
                .as_str()
                .is_some_and(|message| message.contains("格式錯誤")),
            "q={q}：{}",
            body["message"]
        );
    }
}
