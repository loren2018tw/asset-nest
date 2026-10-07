//! 網段設定整合測試：CRUD、CIDR 正規化、重疊與結構驗證（見票 03）。

use axum::body::Body;
use axum::http::{HeaderMap, Method, Request, StatusCode, header};
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

/// 以 `oneshot` 發送 GET；回傳狀態碼、標頭與原始內容（二進位／CSV 回應用）。
async fn send_bytes(
    pool: &SqlitePool,
    method: Method,
    uri: &str,
) -> (StatusCode, HeaderMap, Vec<u8>) {
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .body(Body::empty())
        .expect("建立請求");

    let response = app(AppState::new(
        pool.clone(),
        std::env::temp_dir().join("asset-nest-test-no-dist"),
    ))
    .oneshot(request)
    .await
    .expect("執行請求");

    let status = response.status();
    let headers = response.headers().clone();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("讀取回應內容")
        .to_bytes()
        .to_vec();

    (status, headers, bytes)
}

/// 新增網段並斷言成功，回傳回應 JSON。
async fn create_subnet(pool: &SqlitePool, body: Value) -> Value {
    let (status, json) = send(pool, Method::POST, "/api/v1/subnets", Some(body)).await;
    assert_eq!(status, StatusCode::CREATED, "新增網段應成功：{json}");
    json
}

/// 新增網段並斷言被結構驗證阻擋，回傳錯誤 JSON。
async fn reject_subnet(pool: &SqlitePool, body: Value) -> Value {
    let (status, json) = send(pool, Method::POST, "/api/v1/subnets", Some(body)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "應被結構驗證阻擋：{json}");
    assert_eq!(json["error"], "validation_error");
    json
}

#[tokio::test]
async fn subnet_crud_lifecycle() {
    let pool = test_pool().await;

    // 新增 v4 網段：名稱、備註、gateway、kea_subnet_id、多段 pool
    let (status, created) = send(
        &pool,
        Method::POST,
        "/api/v1/subnets",
        Some(json!({
            "cidr": "10.0.0.0/24",
            "name": "辦公區",
            "note": "三樓",
            "gateway": "10.0.0.1",
            "kea_subnet_id": 10,
            "pools": [
                { "start_ip": "10.0.0.100", "end_ip": "10.0.0.150" },
                { "start_ip": "10.0.0.200", "end_ip": "10.0.0.220" }
            ]
        })),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED);
    let id = created["id"].as_i64().expect("回應含 id");
    assert!(id > 0, "id 為資料庫自增");
    assert_eq!(created["cidr"], "10.0.0.0/24");
    assert_eq!(created["name"], "辦公區");
    assert_eq!(created["note"], "三樓");
    assert_eq!(created["gateway"], "10.0.0.1");
    assert_eq!(created["kea_subnet_id"], 10);
    assert!(
        created["created_at"]
            .as_str()
            .is_some_and(|value| !value.is_empty()),
        "回應含建立時間"
    );

    let pools = created["pools"].as_array().expect("pools 為陣列");
    assert_eq!(pools.len(), 2);
    assert!(pools[0]["id"].as_i64().is_some_and(|value| value > 0));
    assert_eq!(pools[0]["start_ip"], "10.0.0.100");
    assert_eq!(pools[0]["end_ip"], "10.0.0.150");
    assert_eq!(pools[1]["start_ip"], "10.0.0.200");
    assert_eq!(pools[1]["end_ip"], "10.0.0.220");

    // 列表：名稱、CIDR、地址族（統計欄位見票 07）
    let (status, page) = send(&pool, Method::GET, "/api/v1/subnets", None).await;
    assert_eq!(status, StatusCode::OK);
    let items = page["items"].as_array().expect("items 為陣列");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["id"], id);
    assert_eq!(items[0]["cidr"], "10.0.0.0/24");
    assert_eq!(items[0]["name"], "辦公區");
    assert_eq!(items[0]["family"], "ipv4");

    // 詳情含 pools
    let (status, fetched) = send(&pool, Method::GET, &format!("/api/v1/subnets/{id}"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(fetched["pools"].as_array().expect("pools 為陣列").len(), 2);

    // 編輯：未提供的欄位維持原值（pools 未提供時保留）
    let (status, updated) = send(
        &pool,
        Method::PATCH,
        &format!("/api/v1/subnets/{id}"),
        Some(json!({ "name": "辦公區（新）", "note": "四樓" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["name"], "辦公區（新）");
    assert_eq!(updated["note"], "四樓");
    assert_eq!(updated["gateway"], "10.0.0.1", "未提供 gateway 維持原值");
    assert_eq!(
        updated["kea_subnet_id"], 10,
        "未提供 kea_subnet_id 維持原值"
    );
    assert_eq!(updated["pools"].as_array().expect("pools 為陣列").len(), 2);

    // 編輯：顯式 null 清除選填欄位；pools 整批取代（空陣列＝清空）
    let (status, cleared) = send(
        &pool,
        Method::PATCH,
        &format!("/api/v1/subnets/{id}"),
        Some(json!({ "gateway": null, "kea_subnet_id": null, "pools": [] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(cleared["gateway"].is_null());
    assert!(cleared["kea_subnet_id"].is_null());
    assert_eq!(cleared["pools"], json!([]));

    // 編輯：pools 整批取代為一段
    let (status, replaced) = send(
        &pool,
        Method::PATCH,
        &format!("/api/v1/subnets/{id}"),
        Some(json!({ "pools": [{ "start_ip": "10.0.0.50", "end_ip": "10.0.0.60" }] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let pools = replaced["pools"].as_array().expect("pools 為陣列");
    assert_eq!(pools.len(), 1);
    assert_eq!(pools[0]["start_ip"], "10.0.0.50");

    // 刪除：pools 連動刪除
    let (status, body) = send(
        &pool,
        Method::DELETE,
        &format!("/api/v1/subnets/{id}"),
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

    // 404 情境
    let (status, body) = send(&pool, Method::GET, &format!("/api/v1/subnets/{id}"), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"], "not_found");

    let (status, _) = send(
        &pool,
        Method::DELETE,
        &format!("/api/v1/subnets/{id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, _) = send(
        &pool,
        Method::PATCH,
        &format!("/api/v1/subnets/{id}"),
        Some(json!({ "name": "x" })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // 名稱選填、不強制唯一
    let first = create_subnet(&pool, json!({ "cidr": "10.1.0.0/24", "name": "同名" })).await;
    assert!(first["name"] == "同名");
    let second = create_subnet(&pool, json!({ "cidr": "10.2.0.0/24", "name": "同名" })).await;
    assert_eq!(second["name"], "同名", "同名網段可建立");
    assert!(second["kea_subnet_id"].is_null());
    assert_eq!(second["pools"], json!([]));

    // v6 網段：kea_subnet_id 與 pools 皆為空
    let v6 = create_subnet(&pool, json!({ "cidr": "fd00::/64", "name": "內部 v6" })).await;
    assert_eq!(v6["cidr"], "fd00::/64");
    let (_, page) = send(&pool, Method::GET, "/api/v1/subnets", None).await;
    let families: Vec<&str> = page["items"]
        .as_array()
        .expect("items 為陣列")
        .iter()
        .map(|item| item["family"].as_str().expect("family 為字串"))
        .collect();
    assert_eq!(families, vec!["ipv4", "ipv4", "ipv6"]);
}

#[tokio::test]
async fn cidr_is_required_valid_and_normalized() {
    let pool = test_pool().await;

    // 缺 CIDR → 400
    let body = reject_subnet(&pool, json!({ "name": "無 CIDR" })).await;
    assert_eq!(body["message"], "CIDR 為必填");
    assert_eq!(body["details"]["field"], "cidr");

    // 非法 CIDR → 400
    for invalid in ["192.168.1.0", "not-a-cidr", "192.168.1.0/33", "10.0.0.0/-1"] {
        let body = reject_subnet(&pool, json!({ "cidr": invalid })).await;
        assert_eq!(body["details"]["field"], "cidr", "輸入 {invalid}");
    }

    // host bits 收斂為網路地址
    let cases = [
        ("192.168.1.5/24", "192.168.1.0/24"),
        ("192.168.1.255/25", "192.168.1.128/25"),
        ("10.20.30.40/8", "10.0.0.0/8"),
        ("fd00::1234/64", "fd00::/64"),
        ("2001:db8:1:2::99/48", "2001:db8:1::/48"),
    ];
    for (input, expected) in cases {
        let created = create_subnet(&pool, json!({ "cidr": input })).await;
        assert_eq!(created["cidr"], expected, "輸入 {input}");
        // 逐一移除，避免案例之間互相重疊
        let (status, _) = send(
            &pool,
            Method::DELETE,
            &format!("/api/v1/subnets/{}", created["id"]),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
    }

    // 正規化後相同的 CIDR 視為重複（host bits 不同、網路地址相同）
    create_subnet(&pool, json!({ "cidr": "192.168.1.0/24" })).await;
    let body = reject_subnet(&pool, json!({ "cidr": "192.168.1.99/24" })).await;
    assert_eq!(body["details"]["field"], "cidr");
}

#[tokio::test]
async fn overlapping_subnets_are_blocked_in_all_forms() {
    let pool = test_pool().await;
    let existing = create_subnet(&pool, json!({ "cidr": "10.0.0.0/24", "name": "辦公區" })).await;
    let existing_id = existing["id"].as_i64().expect("回應含 id");

    // 完全相同
    let body = reject_subnet(&pool, json!({ "cidr": "10.0.0.0/24" })).await;
    assert_eq!(body["details"]["field"], "cidr");
    assert_eq!(body["details"]["conflict"]["id"], existing_id);
    assert_eq!(body["details"]["conflict"]["cidr"], "10.0.0.0/24");
    assert!(
        body["message"]
            .as_str()
            .is_some_and(|message| message.contains("辦公區")),
        "訊息指出衝突對象：{}",
        body["message"]
    );

    // 新網段包含既有（嵌套，新在外）
    let body = reject_subnet(&pool, json!({ "cidr": "10.0.0.0/16" })).await;
    assert_eq!(body["details"]["conflict"]["id"], existing_id);

    // 既有包含新網段（嵌套，新在內）
    let body = reject_subnet(&pool, json!({ "cidr": "10.0.0.0/25" })).await;
    assert_eq!(body["details"]["conflict"]["id"], existing_id);

    // 未對齊輸入經正規化後仍與既有重疊（CIDR 區塊只會互斥或包含）
    let body = reject_subnet(&pool, json!({ "cidr": "10.0.0.200/26" })).await;
    assert_eq!(body["details"]["conflict"]["id"], existing_id);

    // 相鄰不重疊：可建立
    let adjacent = create_subnet(&pool, json!({ "cidr": "10.0.1.0/24" })).await;
    let adjacent_id = adjacent["id"].as_i64().expect("回應含 id");

    // 編輯時同樣阻擋（改為與他段重疊）
    let (status, body) = send(
        &pool,
        Method::PATCH,
        &format!("/api/v1/subnets/{adjacent_id}"),
        Some(json!({ "cidr": "10.0.0.0/16" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["details"]["field"], "cidr");
    assert_eq!(body["details"]["conflict"]["id"], existing_id);

    // 編輯自身（CIDR 不變）不誤報
    let (status, kept) = send(
        &pool,
        Method::PATCH,
        &format!("/api/v1/subnets/{adjacent_id}"),
        Some(json!({ "cidr": "10.0.1.0/24", "name": "隔壁" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "排除自身：{kept}");
    assert_eq!(kept["name"], "隔壁");

    // v6 重疊亦阻擋（v4 與 v6 互不重疊，可共存）
    create_subnet(&pool, json!({ "cidr": "fd00::/64" })).await;
    let body = reject_subnet(&pool, json!({ "cidr": "fd00::/48" })).await;
    assert_eq!(body["details"]["field"], "cidr");
    create_subnet(&pool, json!({ "cidr": "2001:db8::/64" })).await;
}

#[tokio::test]
async fn gateway_must_be_inside_cidr() {
    let pool = test_pool().await;

    // gateway 不在 CIDR 內 → 400
    let body = reject_subnet(
        &pool,
        json!({ "cidr": "10.0.0.0/24", "gateway": "10.0.1.1" }),
    )
    .await;
    assert_eq!(body["details"]["field"], "gateway");
    assert!(
        body["message"]
            .as_str()
            .is_some_and(|message| message.contains("不在網段")),
        "訊息說明不在網段內：{}",
        body["message"]
    );

    // gateway 格式錯誤 → 400
    let body = reject_subnet(&pool, json!({ "cidr": "10.0.0.0/24", "gateway": "abc" })).await;
    assert_eq!(body["details"]["field"], "gateway");

    // 跨地址族（v6 網段配 v4 gateway）→ 400
    let body = reject_subnet(&pool, json!({ "cidr": "fd00::/64", "gateway": "10.0.0.1" })).await;
    assert_eq!(body["details"]["field"], "gateway");

    // 在 CIDR 內（含網路地址與廣播地址）→ 可建立
    for gateway in ["10.0.0.0", "10.0.0.1", "10.0.0.255"] {
        let created =
            create_subnet(&pool, json!({ "cidr": "10.0.0.0/24", "gateway": gateway })).await;
        assert_eq!(created["gateway"], gateway);
        // 清掉避免下次重疊
        let (status, _) = send(
            &pool,
            Method::DELETE,
            &format!("/api/v1/subnets/{}", created["id"]),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
    }

    // 編輯：gateway 移出 CIDR → 400；顯式 null 清除 → 200
    let subnet = create_subnet(
        &pool,
        json!({ "cidr": "10.9.0.0/24", "gateway": "10.9.0.1" }),
    )
    .await;
    let id = subnet["id"].as_i64().expect("回應含 id");

    let (status, body) = send(
        &pool,
        Method::PATCH,
        &format!("/api/v1/subnets/{id}"),
        Some(json!({ "gateway": "10.9.1.1" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["details"]["field"], "gateway");

    let (status, cleared) = send(
        &pool,
        Method::PATCH,
        &format!("/api/v1/subnets/{id}"),
        Some(json!({ "gateway": null })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(cleared["gateway"].is_null());

    // 僅變更 CIDR 時，未提供的 gateway 以合併後狀態驗證
    let subnet = create_subnet(
        &pool,
        json!({ "cidr": "10.10.0.0/24", "gateway": "10.10.0.10" }),
    )
    .await;
    let id = subnet["id"].as_i64().expect("回應含 id");

    let (status, kept) = send(
        &pool,
        Method::PATCH,
        &format!("/api/v1/subnets/{id}"),
        Some(json!({ "cidr": "10.10.0.0/25" })),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "縮小後 gateway 仍在 CIDR 內：{kept}"
    );
    assert_eq!(kept["gateway"], "10.10.0.10");

    let (status, body) = send(
        &pool,
        Method::PATCH,
        &format!("/api/v1/subnets/{id}"),
        Some(json!({ "cidr": "10.10.0.128/25" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "縮小後 gateway 出界應阻擋");
    assert_eq!(body["details"]["field"], "gateway");
}

#[tokio::test]
async fn pools_must_be_inside_cidr_and_not_overlap() {
    let pool = test_pool().await;

    // pool 不在 CIDR 內 → 400
    let body = reject_subnet(
        &pool,
        json!({
            "cidr": "10.0.0.0/24",
            "pools": [{ "start_ip": "10.0.1.10", "end_ip": "10.0.1.20" }]
        }),
    )
    .await;
    assert_eq!(body["details"]["field"], "pools[0].start_ip");

    // 終點出界（起點在內）→ 400
    let body = reject_subnet(
        &pool,
        json!({
            "cidr": "10.0.0.0/24",
            "pools": [{ "start_ip": "10.0.0.250", "end_ip": "10.0.1.10" }]
        }),
    )
    .await;
    assert_eq!(body["details"]["field"], "pools[0].start_ip");

    // 起點大於終點 → 400
    let body = reject_subnet(
        &pool,
        json!({
            "cidr": "10.0.0.0/24",
            "pools": [{ "start_ip": "10.0.0.20", "end_ip": "10.0.0.10" }]
        }),
    )
    .await;
    assert_eq!(body["details"]["field"], "pools[0].start_ip");

    // 缺少端點 → 400
    let body = reject_subnet(
        &pool,
        json!({
            "cidr": "10.0.0.0/24",
            "pools": [{ "start_ip": "10.0.0.10" }]
        }),
    )
    .await;
    assert_eq!(body["details"]["field"], "pools[0].end_ip");

    // pool 彼此重疊（含共用端點）→ 400，訊息指出兩段 pool
    let body = reject_subnet(
        &pool,
        json!({
            "cidr": "10.0.0.0/24",
            "pools": [
                { "start_ip": "10.0.0.10", "end_ip": "10.0.0.20" },
                { "start_ip": "10.0.0.15", "end_ip": "10.0.0.30" }
            ]
        }),
    )
    .await;
    assert_eq!(body["details"]["field"], "pools");
    assert!(
        body["message"]
            .as_str()
            .is_some_and(|message| message.contains("pools[1]")),
        "訊息指出另一段 pool：{}",
        body["message"]
    );

    let body = reject_subnet(
        &pool,
        json!({
            "cidr": "10.0.0.0/24",
            "pools": [
                { "start_ip": "10.0.0.10", "end_ip": "10.0.0.20" },
                { "start_ip": "10.0.0.20", "end_ip": "10.0.0.30" }
            ]
        }),
    )
    .await;
    assert_eq!(body["details"]["field"], "pools", "共用端點視為重疊");

    // 相鄰不重疊 → 可建立
    let subnet = create_subnet(
        &pool,
        json!({
            "cidr": "10.0.0.0/24",
            "pools": [
                { "start_ip": "10.0.0.10", "end_ip": "10.0.0.19" },
                { "start_ip": "10.0.0.20", "end_ip": "10.0.0.30" }
            ]
        }),
    )
    .await;
    let id = subnet["id"].as_i64().expect("回應含 id");
    assert_eq!(subnet["pools"].as_array().expect("pools 為陣列").len(), 2);

    // 編輯：pool 出界 → 400；清空 → 200
    let (status, body) = send(
        &pool,
        Method::PATCH,
        &format!("/api/v1/subnets/{id}"),
        Some(json!({ "pools": [{ "start_ip": "10.0.0.0", "end_ip": "10.0.2.0" }] })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["details"]["field"], "pools[0].start_ip");

    let (status, cleared) = send(
        &pool,
        Method::PATCH,
        &format!("/api/v1/subnets/{id}"),
        Some(json!({ "pools": [] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cleared["pools"], json!([]));

    // v6 不因 pool 欄位而放行 v4 位址檢查：先驗證 v6 一律拒絕 pool
    let body = reject_subnet(
        &pool,
        json!({
            "cidr": "fd00::/64",
            "pools": [{ "start_ip": "10.0.0.10", "end_ip": "10.0.0.20" }]
        }),
    )
    .await;
    assert_eq!(body["details"]["field"], "pools");
}

#[tokio::test]
async fn v6_rejects_pools_and_kea_subnet_id() {
    let pool = test_pool().await;

    // v6 帶 kea_subnet_id → 400
    let body = reject_subnet(&pool, json!({ "cidr": "fd00::/64", "kea_subnet_id": 1 })).await;
    assert_eq!(body["details"]["field"], "kea_subnet_id");
    assert!(
        body["message"]
            .as_str()
            .is_some_and(|message| message.contains("IPv6")),
        "訊息說明 v6 不支援：{}",
        body["message"]
    );

    // v6 帶 pool → 400
    let body = reject_subnet(
        &pool,
        json!({
            "cidr": "fd00::/64",
            "pools": [{ "start_ip": "fd00::10", "end_ip": "fd00::20" }]
        }),
    )
    .await;
    assert_eq!(body["details"]["field"], "pools");

    // v6 基本建立 → 201
    let v6 = create_subnet(&pool, json!({ "cidr": "fd00::/64" })).await;
    let v6_id = v6["id"].as_i64().expect("回應含 id");
    assert!(v6["kea_subnet_id"].is_null());
    assert_eq!(v6["pools"], json!([]));

    // 編輯 v6：加 kea_subnet_id／pool 皆阻擋
    let (status, body) = send(
        &pool,
        Method::PATCH,
        &format!("/api/v1/subnets/{v6_id}"),
        Some(json!({ "kea_subnet_id": 5 })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["details"]["field"], "kea_subnet_id");

    let (status, body) = send(
        &pool,
        Method::PATCH,
        &format!("/api/v1/subnets/{v6_id}"),
        Some(json!({ "pools": [{ "start_ip": "fd00::10", "end_ip": "fd00::20" }] })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["details"]["field"], "pools");

    // v4 改為 v6 時，殘留的 kea_subnet_id 以合併後狀態阻擋
    let v4 = create_subnet(&pool, json!({ "cidr": "10.7.0.0/24", "kea_subnet_id": 7 })).await;
    let v4_id = v4["id"].as_i64().expect("回應含 id");
    let (status, body) = send(
        &pool,
        Method::PATCH,
        &format!("/api/v1/subnets/{v4_id}"),
        Some(json!({ "cidr": "fd00:7::/64" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["details"]["field"], "kea_subnet_id");

    // v4 改為 v6 時，殘留的 pool 以合併後狀態阻擋
    let with_pool = create_subnet(
        &pool,
        json!({
            "cidr": "10.11.0.0/24",
            "pools": [{ "start_ip": "10.11.0.10", "end_ip": "10.11.0.20" }]
        }),
    )
    .await;
    let with_pool_id = with_pool["id"].as_i64().expect("回應含 id");
    let (status, body) = send(
        &pool,
        Method::PATCH,
        &format!("/api/v1/subnets/{with_pool_id}"),
        Some(json!({ "cidr": "fd00:11::/64" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["details"]["field"], "pools");

    // v6 可以有名稱與備註
    let named = create_subnet(
        &pool,
        json!({ "cidr": "fd00:8::/64", "name": "v6 區", "note": "備註" }),
    )
    .await;
    assert_eq!(named["name"], "v6 區");
    assert_eq!(named["note"], "備註");
}

#[tokio::test]
async fn subnet_exclusions_lifecycle() {
    let pool = test_pool().await;

    // 建立：多段排除範圍（含用途說明；單一位址以起=迄表示）
    let created = create_subnet(
        &pool,
        json!({
            "cidr": "10.0.0.0/24",
            "exclusions": [
                { "start_ip": "10.0.0.30", "end_ip": "10.0.0.40", "note": "NAT 對外" },
                { "start_ip": "10.0.0.50", "end_ip": "10.0.0.50" }
            ]
        }),
    )
    .await;
    let id = created["id"].as_i64().expect("回應含 id");

    let exclusions = created["exclusions"].as_array().expect("exclusions 為陣列");
    assert_eq!(exclusions.len(), 2);
    assert!(exclusions[0]["id"].as_i64().is_some_and(|value| value > 0));
    assert_eq!(exclusions[0]["start_ip"], "10.0.0.30");
    assert_eq!(exclusions[0]["end_ip"], "10.0.0.40");
    assert_eq!(exclusions[0]["note"], "NAT 對外");
    assert_eq!(exclusions[1]["start_ip"], "10.0.0.50");
    assert_eq!(exclusions[1]["end_ip"], "10.0.0.50");
    assert!(exclusions[1]["note"].is_null(), "用途說明選填");

    // 讀回一致（依 id 升冪，即輸入順序）
    let (status, fetched) = send(&pool, Method::GET, &format!("/api/v1/subnets/{id}"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(fetched["exclusions"], created["exclusions"]);

    // PATCH 未提供 exclusions：沿用原值（含用途說明）
    let (status, kept) = send(
        &pool,
        Method::PATCH,
        &format!("/api/v1/subnets/{id}"),
        Some(json!({ "name": "辦公區" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "未提供 exclusions 維持原值：{kept}");
    assert_eq!(
        kept["exclusions"]
            .as_array()
            .expect("exclusions 為陣列")
            .len(),
        2
    );
    assert_eq!(kept["exclusions"][0]["note"], "NAT 對外");

    // PATCH 整批取代
    let (status, replaced) = send(
        &pool,
        Method::PATCH,
        &format!("/api/v1/subnets/{id}"),
        Some(json!({
            "exclusions": [
                { "start_ip": "10.0.0.100", "end_ip": "10.0.0.120", "note": "設備" }
            ]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let exclusions = replaced["exclusions"]
        .as_array()
        .expect("exclusions 為陣列");
    assert_eq!(exclusions.len(), 1);
    assert_eq!(exclusions[0]["start_ip"], "10.0.0.100");
    assert_eq!(exclusions[0]["end_ip"], "10.0.0.120");
    assert_eq!(exclusions[0]["note"], "設備");

    // PATCH 空陣列清空
    let (status, cleared) = send(
        &pool,
        Method::PATCH,
        &format!("/api/v1/subnets/{id}"),
        Some(json!({ "exclusions": [] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cleared["exclusions"], json!([]));
    let remaining: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM subnet_exclusions")
        .fetch_one(&pool)
        .await
        .expect("查詢排除範圍");
    assert_eq!(remaining, 0, "整批取代後資料庫不留舊列");
}

#[tokio::test]
async fn exclusions_reject_pool_overlap_and_v6() {
    let pool = test_pool().await;

    // 與 pool 重疊 → 400 validation_error，field exclusions
    let body = reject_subnet(
        &pool,
        json!({
            "cidr": "10.0.0.0/24",
            "pools": [{ "start_ip": "10.0.0.10", "end_ip": "10.0.0.20" }],
            "exclusions": [{ "start_ip": "10.0.0.15", "end_ip": "10.0.0.25" }]
        }),
    )
    .await;
    assert_eq!(body["details"]["field"], "exclusions");
    assert!(
        body["message"]
            .as_str()
            .is_some_and(|message| message.contains("不得與 DHCP 位址池重疊")),
        "訊息說明與 pool 重疊：{}",
        body["message"]
    );

    // v6 帶排除範圍 → 400
    let body = reject_subnet(
        &pool,
        json!({
            "cidr": "fd00::/64",
            "exclusions": [{ "start_ip": "fd00::10", "end_ip": "fd00::20" }]
        }),
    )
    .await;
    assert_eq!(body["details"]["field"], "exclusions");
    assert!(
        body["message"]
            .as_str()
            .is_some_and(|message| message.contains("IPv6")),
        "訊息說明 v6 不支援：{}",
        body["message"]
    );
}

#[tokio::test]
async fn kea_subnet_id_is_unique_among_v4() {
    let pool = test_pool().await;

    let first = create_subnet(&pool, json!({ "cidr": "10.1.0.0/24", "kea_subnet_id": 10 })).await;
    let first_id = first["id"].as_i64().expect("回應含 id");

    // 重複 kea_subnet_id → 400，details 指出衝突對象
    let body = reject_subnet(&pool, json!({ "cidr": "10.2.0.0/24", "kea_subnet_id": 10 })).await;
    assert_eq!(body["details"]["field"], "kea_subnet_id");
    assert_eq!(body["details"]["conflict"]["id"], first_id);
    assert_eq!(body["details"]["conflict"]["cidr"], "10.1.0.0/24");

    // 不同 kea_subnet_id → 可建立
    let second = create_subnet(&pool, json!({ "cidr": "10.2.0.0/24", "kea_subnet_id": 11 })).await;
    let second_id = second["id"].as_i64().expect("回應含 id");

    // 編輯改為重複 → 400
    let (status, body) = send(
        &pool,
        Method::PATCH,
        &format!("/api/v1/subnets/{second_id}"),
        Some(json!({ "kea_subnet_id": 10 })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["details"]["field"], "kea_subnet_id");
    assert_eq!(body["details"]["conflict"]["id"], first_id);

    // 編輯自身（kea_subnet_id 不變）不誤報
    let (status, kept) = send(
        &pool,
        Method::PATCH,
        &format!("/api/v1/subnets/{second_id}"),
        Some(json!({ "kea_subnet_id": 11, "name": "第二區" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "排除自身：{kept}");
    assert_eq!(kept["kea_subnet_id"], 11);

    // v4 無 kea_subnet_id 可與有者並存
    let none = create_subnet(&pool, json!({ "cidr": "10.3.0.0/24" })).await;
    assert!(none["kea_subnet_id"].is_null());

    // DB UNIQUE 雙保險
    let direct = sqlx::query("UPDATE subnets SET kea_subnet_id = 10 WHERE id = ?")
        .bind(none["id"].as_i64().expect("回應含 id"))
        .execute(&pool)
        .await;
    assert!(direct.is_err(), "資料庫 UNIQUE 應擋下重複 kea_subnet_id");
}

#[tokio::test]
async fn kea_subnet_id_must_be_within_kea_range() {
    let pool = test_pool().await;

    // 上下界允許
    let min = create_subnet(&pool, json!({ "cidr": "10.4.0.0/24", "kea_subnet_id": 1 })).await;
    assert_eq!(min["kea_subnet_id"], 1);
    let max = create_subnet(
        &pool,
        json!({ "cidr": "10.5.0.0/24", "kea_subnet_id": 4294967294_i64 }),
    )
    .await;
    assert_eq!(max["kea_subnet_id"], 4294967294_i64);

    // 超出範圍（0 與 4294967295 皆不合法；見 ADR-0023）
    for (cidr, id) in [("10.6.0.0/24", 0_i64), ("10.7.0.0/24", 4294967295_i64)] {
        let body = reject_subnet(&pool, json!({ "cidr": cidr, "kea_subnet_id": id })).await;
        assert_eq!(body["details"]["field"], "kea_subnet_id", "{body}");
        assert!(
            body["message"]
                .as_str()
                .is_some_and(|message| message.contains("4294967294")),
            "訊息說明範圍：{body}"
        );
    }
}

#[tokio::test]
async fn export_subnets_returns_ordered_csv_with_bom() {
    let pool = test_pool().await;

    // 建立順序刻意打亂，驗證 v4 先、v6 後與同族依網路位址數值排序。
    create_subnet(
        &pool,
        json!({ "cidr": "fd00::/64", "name": "v6 二", "gateway": "fd00::1" }),
    )
    .await;
    create_subnet(&pool, json!({ "cidr": "192.168.0.0/24" })).await;
    create_subnet(
        &pool,
        json!({ "cidr": "2001:db8::/64", "name": "v6 一", "gateway": "2001:db8::1" }),
    )
    .await;
    create_subnet(
        &pool,
        json!({
            "cidr": "10.0.0.0/24",
            "name": "辦公區",
            "note": "三樓,近電梯",
            "gateway": "10.0.0.1",
            "kea_subnet_id": 10,
            "pools": [
                { "start_ip": "10.0.0.100", "end_ip": "10.0.0.150" },
                { "start_ip": "10.0.0.200", "end_ip": "10.0.0.220" }
            ],
            "exclusions": [
                { "start_ip": "10.0.0.30", "end_ip": "10.0.0.40", "note": "NAT 對外" },
                { "start_ip": "10.0.0.50", "end_ip": "10.0.0.50" }
            ]
        }),
    )
    .await;

    let (status, headers, bytes) = send_bytes(&pool, Method::GET, "/api/v1/subnets/export").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "靜態 export 路由優先於 /subnets/{{id}}"
    );
    assert_eq!(headers[header::CONTENT_TYPE], "text/csv; charset=utf-8");

    let date = chrono::Local::now().format("%Y%m%d");
    let disposition = headers[header::CONTENT_DISPOSITION]
        .to_str()
        .expect("Content-Disposition 為文字");
    assert!(disposition.starts_with("attachment"), "{disposition}");
    assert!(
        disposition.contains(&format!(
            "filename*=UTF-8''%E7%B6%B2%E6%AE%B5%E5%8C%AF%E5%87%BA_{date}.csv"
        )),
        "中文檔名以 filename* 編碼：{disposition}"
    );

    assert_eq!(&bytes[..3], &[0xEF, 0xBB, 0xBF], "回應須有 UTF-8 BOM");
    let mut reader = csv::Reader::from_reader(&bytes[3..]);
    let csv_headers: Vec<String> = reader
        .headers()
        .expect("標題列")
        .iter()
        .map(str::to_string)
        .collect();
    assert_eq!(
        csv_headers,
        [
            "名稱",
            "CIDR",
            "Gateway",
            "Kea subnet-id",
            "位址池",
            "排除範圍",
            "備註"
        ]
    );

    let rows: Vec<csv::StringRecord> = reader
        .into_records()
        .map(|record| record.expect("資料列"))
        .collect();
    let cidrs: Vec<&str> = rows
        .iter()
        .map(|row| row.get(1).expect("CIDR 欄"))
        .collect();
    assert_eq!(
        cidrs,
        [
            "10.0.0.0/24",
            "192.168.0.0/24",
            "2001:db8::/64",
            "fd00::/64"
        ],
        "v4 先、v6 後；同族依 CIDR 網路位址數值"
    );

    // v4：完整欄位、多段 pool 以 | 分隔、排除範圍含 # 用途說明、含逗號備註經引號往返
    assert_eq!(rows[0].get(0), Some("辦公區"));
    assert_eq!(rows[0].get(2), Some("10.0.0.1"));
    assert_eq!(rows[0].get(3), Some("10"));
    assert_eq!(
        rows[0].get(4),
        Some("10.0.0.100-10.0.0.150|10.0.0.200-10.0.0.220")
    );
    assert_eq!(
        rows[0].get(5),
        Some("10.0.0.30-10.0.0.40#NAT 對外|10.0.0.50-10.0.0.50"),
        "多段排除範圍以 | 分隔；有用途說明時以 # 接續"
    );
    assert_eq!(rows[0].get(6), Some("三樓,近電梯"));

    // v4：選填欄位缺值為空字串
    for column in [0, 2, 3, 4, 5, 6] {
        assert_eq!(rows[1].get(column), Some(""), "選填欄位缺值留空");
    }

    // v6：Kea subnet-id、位址池與排除範圍留空；名稱／gateway／備註照常輸出
    assert_eq!(rows[2].get(0), Some("v6 一"));
    assert_eq!(rows[2].get(2), Some("2001:db8::1"));
    assert_eq!(rows[2].get(3), Some(""), "v6 的 Kea subnet-id 留空");
    assert_eq!(rows[2].get(4), Some(""), "v6 的位址池留空");
    assert_eq!(rows[2].get(5), Some(""), "v6 的排除範圍留空");
    assert_eq!(rows[3].get(0), Some("v6 二"));
    assert_eq!(rows[3].get(2), Some("fd00::1"));
    assert_eq!(rows[3].get(3), Some(""));
    assert_eq!(rows[3].get(4), Some(""));
    assert_eq!(rows[3].get(5), Some(""), "v6 的排除範圍留空");
}
