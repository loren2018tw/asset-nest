//! 衝突標記與網段統計整合測試：IpInPool、IpOutOfSubnet、DuplicateHwAddress
//! 的觸發與不阻擋行為、刪除後重算，以及已用／總數／衝突數（見票 07）。

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

/// 編輯網段（不檢查狀態碼，供各測試自行斷言）。
async fn patch_subnet(pool: &SqlitePool, id: i64, body: Value) -> (StatusCode, Value) {
    send(
        pool,
        Method::PATCH,
        &format!("/api/v1/subnets/{id}"),
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

/// v6 登錄（不檢查狀態碼）。
async fn register_ip(pool: &SqlitePool, subnet_id: i64, body: Value) -> (StatusCode, Value) {
    send(
        pool,
        Method::POST,
        &format!("/api/v1/subnets/{subnet_id}/ips"),
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

/// 讀取網段清單中某網段的摘要。
async fn subnet_summary(pool: &SqlitePool, id: i64) -> Value {
    let (status, json) = send(pool, Method::GET, "/api/v1/subnets", None).await;
    assert_eq!(status, StatusCode::OK, "讀取網段清單應成功：{json}");
    json["items"]
        .as_array()
        .expect("items 為陣列")
        .iter()
        .find(|item| item["id"] == id)
        .unwrap_or_else(|| panic!("清單含網段 {id}"))
        .clone()
}

/// 指派列總數（直接查資料庫）。
async fn assignment_count(pool: &SqlitePool) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM ip_assignments")
        .fetch_one(pool)
        .await
        .expect("查詢指派筆數")
}

#[tokio::test]
async fn ip_in_pool_is_flagged_after_pool_expansion_without_blocking() {
    let pool = test_pool().await;
    let asset_id = create_asset(&pool, "資料庫主機", "機房 A").await;
    let interface_id = create_interface(
        &pool,
        asset_id,
        json!({ "name": "eth0", "mac": "aa:bb:cc:dd:ee:01" }),
    )
    .await;
    let subnet_id = create_subnet(&pool, json!({ "cidr": "10.0.0.0/29" })).await;

    // 指派當下不在 pool 內：無警示
    let (status, created) = put_assignment(
        &pool,
        subnet_id,
        "10.0.0.1",
        json!({ "interface_id": interface_id, "purpose": "static" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(created["warnings"], json!([]));

    // 擴大 pool 涵蓋既有指派：不阻擋（落池改以衝突標記呈現）
    let (status, updated) = patch_subnet(
        &pool,
        subnet_id,
        json!({ "pools": [{ "start_ip": "10.0.0.1", "end_ip": "10.0.0.2" }] }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "擴大 pool 不阻擋：{updated}");

    // IP 列：IpInPool 徽章；狀態仍以用途為準
    let page = list_ips(&pool, subnet_id, "").await;
    let assigned = row(&page, "10.0.0.1");
    assert_eq!(assigned["conflicts"], json!(["IpInPool"]));
    assert_eq!(assigned["in_pool"], true);
    assert_eq!(assigned["status"], "static");

    // 更新既有指派（改用途）：附警示、不阻擋（落池位址不再被結構驗證擋下）
    let (status, updated) = put_assignment(
        &pool,
        subnet_id,
        "10.0.0.1",
        json!({ "interface_id": interface_id, "purpose": "reservation" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "衝突不阻擋儲存：{updated}");
    let warnings = updated["warnings"].as_array().expect("warnings 為陣列");
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0]["code"], "IpInPool");
    assert!(
        warnings[0]["message"]
            .as_str()
            .is_some_and(|message| message.contains("池內")),
        "訊息說明落池：{}",
        warnings[0]["message"]
    );

    // 網段統計：已用 1／總數 6／衝突 1
    let summary = subnet_summary(&pool, subnet_id).await;
    assert_eq!(summary["used"], 1);
    assert_eq!(summary["total"], 6);
    assert_eq!(summary["conflicts"], 1);
}

#[tokio::test]
async fn v4_out_of_subnet_is_flagged_after_shrink_and_stays_visible() {
    let pool = test_pool().await;
    let asset_id = create_asset(&pool, "出界主機", "機房 A").await;
    let interface_id = create_interface(&pool, asset_id, json!({ "name": "eth0" })).await;
    let subnet_id = create_subnet(&pool, json!({ "cidr": "10.0.0.0/24" })).await;

    let (status, created) = put_assignment(
        &pool,
        subnet_id,
        "10.0.0.200",
        json!({ "interface_id": interface_id, "purpose": "static" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(created["warnings"], json!([]));

    // 縮小 CIDR 使既有指派（.200）出界：不阻擋
    let (status, updated) = patch_subnet(&pool, subnet_id, json!({ "cidr": "10.0.0.0/25" })).await;
    assert_eq!(status, StatusCode::OK, "縮小 CIDR 不阻擋：{updated}");
    assert_eq!(updated["cidr"], "10.0.0.0/25");

    // 清單聯集：host 範圍列＋出界指派列；出界列仍可見且標記
    let page = list_ips(&pool, subnet_id, "?per_page=200").await;
    assert_eq!(page["total"], 127, "126 個 host 加 1 筆出界指派");
    assert_eq!(page["items"].as_array().expect("items 為陣列").len(), 127);
    let out = row(&page, "10.0.0.200");
    assert_eq!(out["conflicts"], json!(["IpOutOfSubnet"]));
    assert_eq!(out["status"], "static");
    assert_eq!(out["assignment"]["asset_description"], "出界主機");

    // 精確搜尋、狀態篩選與分頁皆涵蓋出界列
    let page = list_ips(&pool, subnet_id, "?q=10.0.0.200").await;
    assert_eq!(page["total"], 1);
    assert_eq!(page["items"][0]["conflicts"], json!(["IpOutOfSubnet"]));

    let page = list_ips(&pool, subnet_id, "?status=static").await;
    assert_eq!(page["total"], 1);
    assert_eq!(page["items"][0]["address"], "10.0.0.200");

    let page = list_ips(&pool, subnet_id, "?status=available").await;
    assert_eq!(page["total"], 126);

    let page = list_ips(&pool, subnet_id, "?page=3&per_page=50").await;
    assert_eq!(page["items"].as_array().expect("items 為陣列").len(), 27);
    assert_eq!(page["items"][26]["address"], "10.0.0.200");

    // 更新既有指派：附 IpOutOfSubnet 警示、不阻擋
    let (status, updated) = put_assignment(
        &pool,
        subnet_id,
        "10.0.0.200",
        json!({ "interface_id": interface_id, "purpose": "static" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "出界位址更新不阻擋：{updated}");
    assert_eq!(updated["warnings"][0]["code"], "IpOutOfSubnet");

    // 統計：已用 1／總數 126／衝突 1
    let summary = subnet_summary(&pool, subnet_id).await;
    assert_eq!(summary["used"], 1);
    assert_eq!(summary["total"], 126);
    assert_eq!(summary["conflicts"], 1);
}

#[tokio::test]
async fn v6_out_of_subnet_is_flagged_after_shrink() {
    let pool = test_pool().await;
    let asset_id = create_asset(&pool, "v6 主機", "機房 A").await;
    let first_interface = create_interface(&pool, asset_id, json!({ "name": "eth0" })).await;
    let second_interface = create_interface(&pool, asset_id, json!({ "name": "eth1" })).await;
    let subnet_id = create_subnet(&pool, json!({ "cidr": "fd00::/64" })).await;

    let (status, created) = register_ip(
        &pool,
        subnet_id,
        json!({ "address": "fd00::5", "interface_id": first_interface }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(created["warnings"], json!([]), "登錄當下無衝突");

    // 縮小 CIDR 使已登錄位址出界：不阻擋
    let (status, updated) =
        patch_subnet(&pool, subnet_id, json!({ "cidr": "fd00:0:0:1::/64" })).await;
    assert_eq!(status, StatusCode::OK, "v6 縮小不阻擋：{updated}");

    let page = list_ips(&pool, subnet_id, "").await;
    assert_eq!(page["total"], 1, "出界登錄位址仍列出");
    let out = row(&page, "fd00::5");
    assert_eq!(out["conflicts"], json!(["IpOutOfSubnet"]));
    assert_eq!(out["status"], "static");

    // 出界位址更新（PUT）：附警示、不阻擋
    let (status, updated) = put_assignment(
        &pool,
        subnet_id,
        "fd00::5",
        json!({ "interface_id": first_interface, "purpose": "static" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "v6 出界更新不阻擋：{updated}");
    assert_eq!(updated["warnings"][0]["code"], "IpOutOfSubnet");

    // 新登錄網段內位址：無衝突
    let (status, _) = register_ip(
        &pool,
        subnet_id,
        json!({ "address": "fd00:0:0:1::5", "interface_id": second_interface }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let page = list_ips(&pool, subnet_id, "").await;
    assert_eq!(page["total"], 2);
    assert_eq!(row(&page, "fd00:0:0:1::5")["conflicts"], json!([]));

    // v6 統計：used／total 皆為已登錄數
    let summary = subnet_summary(&pool, subnet_id).await;
    assert_eq!(summary["family"], "ipv6");
    assert_eq!(summary["used"], 2);
    assert_eq!(summary["total"], 2, "v6 總數顯示已登錄數");
    assert_eq!(summary["conflicts"], 1);
}

#[tokio::test]
async fn duplicate_hw_address_flagged_and_cleared_on_cancel() {
    let pool = test_pool().await;
    let mac = "aa:bb:cc:dd:ee:ff";
    let first_asset = create_asset(&pool, "設備一", "機房 A").await;
    let second_asset = create_asset(&pool, "設備二", "機房 A").await;
    let third_asset = create_asset(&pool, "設備三", "機房 A").await;
    let first_interface =
        create_interface(&pool, first_asset, json!({ "name": "eth0", "mac": mac })).await;
    let second_interface =
        create_interface(&pool, second_asset, json!({ "name": "eth0", "mac": mac })).await;
    let third_interface =
        create_interface(&pool, third_asset, json!({ "name": "eth0", "mac": mac })).await;
    let subnet_id = create_subnet(&pool, json!({ "cidr": "10.0.0.0/29" })).await;

    // 第一筆保留：同 MAC 尚無其他保留，無警示
    let (status, created) = put_assignment(
        &pool,
        subnet_id,
        "10.0.0.1",
        json!({ "interface_id": first_interface, "purpose": "reservation", "hostname": "host-1" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(created["warnings"], json!([]));

    // 第二筆同 MAC 保留：警示但不阻擋
    let (status, created) = put_assignment(
        &pool,
        subnet_id,
        "10.0.0.2",
        json!({ "interface_id": second_interface, "purpose": "reservation" }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "DuplicateHwAddress 不阻擋：{created}"
    );
    let warnings = created["warnings"].as_array().expect("warnings 為陣列");
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0]["code"], "DuplicateHwAddress");
    assert!(
        warnings[0]["message"]
            .as_str()
            .is_some_and(|message| message.contains(mac)),
        "訊息含 MAC：{}",
        warnings[0]["message"]
    );
    assert_eq!(assignment_count(&pool).await, 2, "兩筆保留皆已儲存");

    // 兩列皆標記；衝突數＝命中筆數 2
    let page = list_ips(&pool, subnet_id, "").await;
    assert_eq!(
        row(&page, "10.0.0.1")["conflicts"],
        json!(["DuplicateHwAddress"])
    );
    assert_eq!(
        row(&page, "10.0.0.2")["conflicts"],
        json!(["DuplicateHwAddress"])
    );
    let summary = subnet_summary(&pool, subnet_id).await;
    assert_eq!(summary["used"], 2);
    assert_eq!(summary["conflicts"], 2);

    // 同 MAC 的手動設定不觸發（規則僅限多筆保留）
    let (status, created) = put_assignment(
        &pool,
        subnet_id,
        "10.0.0.3",
        json!({ "interface_id": third_interface, "purpose": "static" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(created["warnings"], json!([]));
    assert_eq!(
        row(&list_ips(&pool, subnet_id, "").await, "10.0.0.3")["conflicts"],
        json!([])
    );

    // 第二筆改為手動設定：只剩一筆保留，衝突消失
    let (status, updated) = put_assignment(
        &pool,
        subnet_id,
        "10.0.0.2",
        json!({ "interface_id": second_interface, "purpose": "static" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["warnings"], json!([]));
    assert_eq!(
        row(&list_ips(&pool, subnet_id, "").await, "10.0.0.1")["conflicts"],
        json!([])
    );
    assert_eq!(subnet_summary(&pool, subnet_id).await["conflicts"], 0);

    // 改回保留：衝突再現
    let (status, _) = put_assignment(
        &pool,
        subnet_id,
        "10.0.0.2",
        json!({ "interface_id": second_interface, "purpose": "reservation" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(subnet_summary(&pool, subnet_id).await["conflicts"], 2);

    // 取消第二筆：第一筆的衝突隨之消失（刪除後重算）
    let (status, body) = delete_assignment(&pool, subnet_id, "10.0.0.2").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({}));
    let page = list_ips(&pool, subnet_id, "").await;
    assert_eq!(row(&page, "10.0.0.1")["conflicts"], json!([]));
    assert_eq!(subnet_summary(&pool, subnet_id).await["conflicts"], 0);
}

#[tokio::test]
async fn conflict_count_counts_rows_not_rules() {
    let pool = test_pool().await;
    let mac = "aa:bb:cc:dd:ee:01";
    let first_asset = create_asset(&pool, "設備一", "機房 A").await;
    let second_asset = create_asset(&pool, "設備二", "機房 A").await;
    let first_interface =
        create_interface(&pool, first_asset, json!({ "name": "eth0", "mac": mac })).await;
    let second_interface =
        create_interface(&pool, second_asset, json!({ "name": "eth0", "mac": mac })).await;
    let subnet_id = create_subnet(&pool, json!({ "cidr": "10.0.0.0/28" })).await;

    for (address, interface_id) in [
        ("10.0.0.10", first_interface),
        ("10.0.0.11", second_interface),
    ] {
        let (status, _) = put_assignment(
            &pool,
            subnet_id,
            address,
            json!({ "interface_id": interface_id, "purpose": "reservation" }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
    }

    // 縮小後兩筆同 MAC 保留同時出界：每筆命中 2 條規則
    let (status, _) = patch_subnet(&pool, subnet_id, json!({ "cidr": "10.0.0.0/29" })).await;
    assert_eq!(status, StatusCode::OK);

    let page = list_ips(&pool, subnet_id, "").await;
    for address in ["10.0.0.10", "10.0.0.11"] {
        assert_eq!(
            row(&page, address)["conflicts"],
            json!(["IpOutOfSubnet", "DuplicateHwAddress"]),
            "{address} 兩條規則並存"
        );
    }

    // 衝突數＝命中筆數（2），非規則命中數（4）
    let summary = subnet_summary(&pool, subnet_id).await;
    assert_eq!(summary["used"], 2);
    assert_eq!(summary["total"], 6);
    assert_eq!(summary["conflicts"], 2);
}

#[tokio::test]
async fn subnet_summary_stats_cover_slash31_slash32_and_v6() {
    let pool = test_pool().await;
    let first_asset = create_asset(&pool, "主機一", "機房 A").await;
    let first_interface = create_interface(&pool, first_asset, json!({ "name": "eth0" })).await;
    let second_asset = create_asset(&pool, "主機二", "機房 A").await;
    let second_interface = create_interface(&pool, second_asset, json!({ "name": "eth0" })).await;

    let slash31 = create_subnet(&pool, json!({ "cidr": "10.0.1.0/31" })).await;
    let slash32 = create_subnet(&pool, json!({ "cidr": "10.0.2.7/32" })).await;
    let empty_v4 = create_subnet(&pool, json!({ "cidr": "10.0.3.0/30" })).await;
    let v6 = create_subnet(&pool, json!({ "cidr": "fd00::/64" })).await;

    // 同一介面跨網段可各有一筆（ADR-0005）
    let (status, _) = put_assignment(
        &pool,
        slash31,
        "10.0.1.1",
        json!({ "interface_id": first_interface, "purpose": "static" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "/31 全列，端點可指派");
    let (status, _) = put_assignment(
        &pool,
        slash32,
        "10.0.2.7",
        json!({ "interface_id": second_interface, "purpose": "static" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "/32 單一位址可指派");

    for (address, interface_id) in [
        ("fd00::10", first_interface),
        ("fd00::20", second_interface),
    ] {
        let (status, _) = register_ip(
            &pool,
            v6,
            json!({ "address": address, "interface_id": interface_id }),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
    }

    // v4：總數為 host 數（/31 全列、/32 全列）
    let summary = subnet_summary(&pool, slash31).await;
    assert_eq!(summary["family"], "ipv4");
    assert_eq!(summary["used"], 1);
    assert_eq!(summary["total"], 2);
    assert_eq!(summary["conflicts"], 0);

    let summary = subnet_summary(&pool, slash32).await;
    assert_eq!(summary["used"], 1);
    assert_eq!(summary["total"], 1);
    assert_eq!(summary["conflicts"], 0);

    let summary = subnet_summary(&pool, empty_v4).await;
    assert_eq!(summary["used"], 0);
    assert_eq!(summary["total"], 2, "/30 扣 network/broadcast");
    assert_eq!(summary["conflicts"], 0);

    // v6：已用與總數皆為已登錄數
    let summary = subnet_summary(&pool, v6).await;
    assert_eq!(summary["family"], "ipv6");
    assert_eq!(summary["used"], 2);
    assert_eq!(summary["total"], 2, "v6 總數顯示已登錄數");
    assert_eq!(summary["conflicts"], 0);
}
