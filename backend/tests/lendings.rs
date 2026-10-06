//! 借還整合測試：建立→歸還往返、重複借出與重複歸還防護、出借中／已歸還
//! 清單、借用人建議與分頁（見票 02、asset-lending spec §4、§8）。

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

/// 新增資產並斷言成功，回傳回應 JSON。
async fn create_asset(pool: &SqlitePool, property_no: &str, description: &str) -> Value {
    let (status, json) = send(
        pool,
        Method::POST,
        "/api/v1/assets",
        Some(json!({
            "property_no": property_no,
            "description": description,
            "location": "機房 A"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "新增資產應成功：{json}");
    json
}

/// 建立借出並斷言成功，回傳回應 JSON。
async fn create_lending(pool: &SqlitePool, asset_id: i64, body: Value) -> Value {
    let (status, json) = send(
        pool,
        Method::POST,
        &format!("/api/v1/assets/{asset_id}/lendings"),
        Some(body),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "建立借出應成功：{json}");
    json
}

/// 歸還借出紀錄並斷言成功，回傳回應 JSON。
async fn return_lending(pool: &SqlitePool, lending_id: i64) -> Value {
    let (status, json) = send(
        pool,
        Method::POST,
        &format!("/api/v1/lendings/{lending_id}/return"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "歸還應成功：{json}");
    json
}

#[tokio::test]
async fn create_then_return_roundtrip_through_lists() {
    let pool = test_pool().await;

    let asset = create_asset(&pool, "PC-001", "測試筆電").await;
    let asset_id = asset["id"].as_i64().expect("回應含 id");

    // 建立借出：預計歸還日已過 → 逾期；借出時間由伺服器產生（UTC ISO8601）
    let created = create_lending(
        &pool,
        asset_id,
        json!({ "borrower": "王小明", "due_at": "2000-01-01", "note": "測試備註" }),
    )
    .await;
    let lending_id = created["id"].as_i64().expect("回應含 id");
    assert_eq!(created["asset_id"], asset_id);
    assert_eq!(created["borrower"], "王小明");
    assert_eq!(created["due_at"], "2000-01-01");
    assert_eq!(created["note"], "測試備註");
    assert_eq!(created["overdue"], true, "預計歸還日已過應逾期");
    assert!(created["returned_at"].is_null(), "建立時尚未歸還");
    assert!(
        created["lent_at"]
            .as_str()
            .is_some_and(|value| value.ends_with('Z')),
        "借出時間為伺服器 UTC ISO8601"
    );

    // 出借中清單：含資產欄位與逾期旗標；不分頁（無 total／page／per_page）
    let (status, body) = send(&pool, Method::GET, "/api/v1/lendings?returned=false", None).await;
    assert_eq!(status, StatusCode::OK);
    let items = body["items"].as_array().expect("items 為陣列");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["id"], lending_id);
    assert_eq!(items[0]["property_no"], "PC-001");
    assert_eq!(items[0]["description"], "測試筆電");
    assert_eq!(items[0]["borrower"], "王小明");
    assert_eq!(items[0]["overdue"], true);
    assert!(body.get("total").is_none(), "出借中清單不分頁、無 total");
    assert!(body.get("page").is_none());
    assert!(body.get("per_page").is_none());

    // 未帶 returned 參數視為出借中清單
    let (_, body) = send(&pool, Method::GET, "/api/v1/lendings", None).await;
    assert_eq!(body["items"].as_array().expect("items 為陣列").len(), 1);

    // 已歸還清單此時為空
    let (_, body) = send(&pool, Method::GET, "/api/v1/lendings?returned=true", None).await;
    assert_eq!(body["total"], 0);
    assert!(body["items"].as_array().expect("items 為陣列").is_empty());

    // 歸還：returned_at 非空、不再逾期
    let returned = return_lending(&pool, lending_id).await;
    assert!(
        returned["returned_at"]
            .as_str()
            .is_some_and(|value| value.ends_with('Z')),
        "歸還時間非空且為 UTC ISO8601"
    );
    assert_eq!(returned["overdue"], false, "歸還後不應逾期");

    // 出借中清單清空；已歸還清單出現該紀錄（含資產欄位）
    let (_, body) = send(&pool, Method::GET, "/api/v1/lendings?returned=false", None).await;
    assert!(body["items"].as_array().expect("items 為陣列").is_empty());
    let (_, body) = send(&pool, Method::GET, "/api/v1/lendings?returned=true", None).await;
    assert_eq!(body["total"], 1);
    let items = body["items"].as_array().expect("items 為陣列");
    assert_eq!(items[0]["id"], lending_id);
    assert_eq!(items[0]["property_no"], "PC-001");
    assert!(items[0]["returned_at"].is_string());
}

#[tokio::test]
async fn duplicate_lending_and_duplicate_return_are_rejected() {
    let pool = test_pool().await;

    let asset = create_asset(&pool, "", "重複借出主機").await;
    let asset_id = asset["id"].as_i64().expect("回應含 id");
    let lending = create_lending(&pool, asset_id, json!({ "borrower": "王小明" })).await;
    let lending_id = lending["id"].as_i64().expect("回應含 id");

    // 出借中不可再次借出 → 409
    let (status, body) = send(
        &pool,
        Method::POST,
        &format!("/api/v1/assets/{asset_id}/lendings"),
        Some(json!({ "borrower": "陳大頭" })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"], "conflict");
    assert_eq!(body["message"], "此資產已在出借中");

    // 歸還後可再次借出
    return_lending(&pool, lending_id).await;
    create_lending(&pool, asset_id, json!({ "borrower": "陳大頭" })).await;

    // 已歸還的紀錄不可再次歸還 → 409
    let (status, body) = send(
        &pool,
        Method::POST,
        &format!("/api/v1/lendings/{lending_id}/return"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"], "conflict");
    assert_eq!(body["message"], "此借出紀錄已歸還");

    // 不存在的紀錄 → 404
    let (status, body) = send(&pool, Method::POST, "/api/v1/lendings/9999/return", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"], "not_found");
}

#[tokio::test]
async fn create_validates_borrower_due_at_and_asset_existence() {
    let pool = test_pool().await;

    let asset = create_asset(&pool, "", "驗證主機").await;
    let asset_id = asset["id"].as_i64().expect("回應含 id");

    // 借用人缺漏或空白 → 400（field borrower）
    for body in [json!({}), json!({ "borrower": "   " })] {
        let (status, error) = send(
            &pool,
            Method::POST,
            &format!("/api/v1/assets/{asset_id}/lendings"),
            Some(body),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(error["error"], "validation_error");
        assert_eq!(error["details"]["field"], "borrower");
    }

    // 預計歸還日格式錯誤 → 400（field due_at）
    let (status, error) = send(
        &pool,
        Method::POST,
        &format!("/api/v1/assets/{asset_id}/lendings"),
        Some(json!({ "borrower": "王小明", "due_at": "2026/1/1" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error["details"]["field"], "due_at");

    // 驗證失敗不寫入任何紀錄
    let (_, body) = send(&pool, Method::GET, "/api/v1/lendings?returned=false", None).await;
    assert!(body["items"].as_array().expect("items 為陣列").is_empty());

    // 資產不存在 → 404（合法輸入仍不寫入）
    let (status, error) = send(
        &pool,
        Method::POST,
        "/api/v1/assets/9999/lendings",
        Some(json!({ "borrower": "王小明" })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(error["error"], "not_found");
}

#[tokio::test]
async fn returned_list_paginates_with_defaults_and_bounds() {
    let pool = test_pool().await;

    // 7 筆已歸還＋1 筆出借中（不列入已歸還清單）
    let mut returned_ids = Vec::new();
    for index in 0..7 {
        let asset = create_asset(&pool, "", &format!("已歸還資產 {index}")).await;
        let asset_id = asset["id"].as_i64().expect("回應含 id");
        let lending = create_lending(&pool, asset_id, json!({ "borrower": "王小明" })).await;
        let lending_id = lending["id"].as_i64().expect("回應含 id");
        returned_ids.push(lending_id);
        return_lending(&pool, lending_id).await;
    }
    let open_asset = create_asset(&pool, "", "出借中資產").await;
    let open_asset_id = open_asset["id"].as_i64().expect("回應含 id");
    create_lending(&pool, open_asset_id, json!({ "borrower": "陳大頭" })).await;

    // 第一頁：page／per_page／total 正確；借出時間倒序（同秒以 id 倒序決勝）
    let (status, body) = send(
        &pool,
        Method::GET,
        "/api/v1/lendings?returned=true&page=1&per_page=3",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], 7);
    assert_eq!(body["page"], 1);
    assert_eq!(body["per_page"], 3);
    let items = body["items"].as_array().expect("items 為陣列");
    assert_eq!(items.len(), 3);
    let ids: Vec<i64> = items
        .iter()
        .map(|item| item["id"].as_i64().expect("回應含 id"))
        .collect();
    assert_eq!(ids, vec![returned_ids[6], returned_ids[5], returned_ids[4]]);

    // 最後一頁與越界頁
    let (_, body) = send(
        &pool,
        Method::GET,
        "/api/v1/lendings?returned=true&page=3&per_page=3",
        None,
    )
    .await;
    assert_eq!(body["total"], 7);
    assert_eq!(body["items"].as_array().expect("items 為陣列").len(), 1);
    let (_, body) = send(
        &pool,
        Method::GET,
        "/api/v1/lendings?returned=true&page=4&per_page=3",
        None,
    )
    .await;
    assert_eq!(body["total"], 7);
    assert!(
        body["items"].as_array().expect("items 為陣列").is_empty(),
        "越界頁回空 items"
    );

    // 預設每頁 10、page 由 1 起（0 視為第 1 頁）
    let (_, body) = send(&pool, Method::GET, "/api/v1/lendings?returned=true", None).await;
    assert_eq!(body["per_page"], 10);
    assert_eq!(body["page"], 1);
    let (_, body) = send(
        &pool,
        Method::GET,
        "/api/v1/lendings?returned=true&page=0",
        None,
    )
    .await;
    assert_eq!(body["page"], 1, "page=0 視為第 1 頁");

    // 每頁上限 100
    let (_, body) = send(
        &pool,
        Method::GET,
        "/api/v1/lendings?returned=true&per_page=500",
        None,
    )
    .await;
    assert_eq!(body["per_page"], 100, "per_page 夾至上限 100");

    // 出借中清單不分頁：仍只列出 1 筆
    let (_, body) = send(&pool, Method::GET, "/api/v1/lendings?returned=false", None).await;
    assert_eq!(body["items"].as_array().expect("items 為陣列").len(), 1);
}

#[tokio::test]
async fn borrowers_are_distinct_and_ordered_by_most_recent() {
    let pool = test_pool().await;

    // 王小明：借出→歸還（最早）；陳大頭：出借中；Wang→歸還→小寫 wang 再借出（最近）
    let asset_a = create_asset(&pool, "", "A").await;
    let asset_b = create_asset(&pool, "", "B").await;
    let asset_c = create_asset(&pool, "", "C").await;
    let asset_d = create_asset(&pool, "", "D").await;
    let a = asset_a["id"].as_i64().expect("回應含 id");
    let b = asset_b["id"].as_i64().expect("回應含 id");
    let c = asset_c["id"].as_i64().expect("回應含 id");
    let d = asset_d["id"].as_i64().expect("回應含 id");

    let first = create_lending(&pool, a, json!({ "borrower": "王小明" })).await;
    return_lending(&pool, first["id"].as_i64().expect("回應含 id")).await;
    create_lending(&pool, b, json!({ "borrower": "陳大頭" })).await;
    let wang = create_lending(&pool, c, json!({ "borrower": "Wang" })).await;
    return_lending(&pool, wang["id"].as_i64().expect("回應含 id")).await;
    create_lending(&pool, d, json!({ "borrower": "wang" })).await;

    let (status, body) = send(&pool, Method::GET, "/api/v1/lendings/borrowers", None).await;
    assert_eq!(status, StatusCode::OK);
    let borrowers: Vec<&str> = body["items"]
        .as_array()
        .expect("items 為陣列")
        .iter()
        .map(|value| value.as_str().expect("借用人為字串"))
        .collect();
    assert_eq!(
        borrowers,
        vec!["wang", "陳大頭", "王小明"],
        "最近借出者在前、不分大小寫去重（保留最近原文）"
    );

    // 空庫：空陣列
    let empty_pool = test_pool().await;
    let (status, body) = send(&empty_pool, Method::GET, "/api/v1/lendings/borrowers", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["items"].as_array().expect("items 為陣列").is_empty());
}
