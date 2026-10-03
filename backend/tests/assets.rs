//! 資產管理整合測試：CRUD、必填驗證、搜尋與篩選、屆齡計算（見票 01）。

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use sqlx::SqlitePool;
use sqlx::sqlite::SqlitePoolOptions;
use tower::ServiceExt;

use asset_nest::{AppState, app};

/// 建立測試資料庫並套用 migrations（骨架測試未套；領域測試必須先套）。
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
async fn create_asset(pool: &SqlitePool, body: Value) -> Value {
    let (status, json) = send(pool, Method::POST, "/api/v1/assets", Some(body)).await;
    assert_eq!(status, StatusCode::CREATED, "新增資產應成功：{json}");
    json
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

#[tokio::test]
async fn create_read_update_delete_asset() {
    let pool = test_pool().await;

    let (status, created) = send(
        &pool,
        Method::POST,
        "/api/v1/assets",
        Some(json!({
            "property_no": "A-001",
            "description": "測試伺服器",
            "location": "機房 A",
            "device_serial": "SN-001",
            "brand": "Dell",
            "model": "R740",
            "purchase_date": "2000-01-01",
            "lifespan_years": 5,
            "note": "測試用"
        })),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED);
    let id = created["id"].as_i64().expect("回應含 id");
    assert!(id > 0, "id 為資料庫自增");
    assert_eq!(created["description"], "測試伺服器");
    assert_eq!(created["location"], "機房 A");
    assert_eq!(created["brand"], "Dell");
    assert_eq!(created["expired"], true, "2000 年購置＋5 年已屆齡");
    assert!(
        created["created_at"]
            .as_str()
            .is_some_and(|v| !v.is_empty()),
        "回應含建立時間"
    );

    let (status, fetched) = send(&pool, Method::GET, &format!("/api/v1/assets/{id}"), None).await;
    assert_eq!(status, StatusCode::OK);
    // 詳情在資產欄位之外另含 interfaces（票 02 擴充，見 spec §5）
    assert_eq!(
        fetched["description"], created["description"],
        "讀回內容與新增一致"
    );
    assert_eq!(fetched["created_at"], created["created_at"]);
    assert_eq!(fetched["interfaces"], json!([]), "新增資產尚無介面");

    let (status, updated) = send(
        &pool,
        Method::PATCH,
        &format!("/api/v1/assets/{id}"),
        Some(json!({
            "description": "測試伺服器（改名）",
            "location": "機房 B",
            "purchase_date": "2020-01-01",
            "lifespan_years": 50
        })),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["description"], "測試伺服器（改名）");
    assert_eq!(updated["location"], "機房 B");
    assert_eq!(updated["expired"], false, "未逾年限");
    assert_eq!(updated["property_no"], "A-001", "未提供的欄位維持原值");
    assert_eq!(updated["brand"], "Dell", "未提供的欄位維持原值");

    let (status, cleared) = send(
        &pool,
        Method::PATCH,
        &format!("/api/v1/assets/{id}"),
        Some(json!({ "brand": null, "note": "  " })),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert!(cleared["brand"].is_null(), "顯式 null 清除選填欄位");
    assert!(cleared["note"].is_null(), "空白字串視為清除");
    assert_eq!(cleared["description"], "測試伺服器（改名）");

    let (status, body) = send(&pool, Method::DELETE, &format!("/api/v1/assets/{id}"), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(body, Value::Null);

    let (status, body) = send(&pool, Method::GET, &format!("/api/v1/assets/{id}"), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"], "not_found");

    let (status, _) = send(&pool, Method::DELETE, &format!("/api/v1/assets/{id}"), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "刪除不存在的資產回 404");

    // id 單調遞增（AUTOINCREMENT）
    let second = create_asset(
        &pool,
        json!({ "description": "第二台", "location": "機房 A" }),
    )
    .await;
    assert!(
        second["id"].as_i64().expect("回應含 id") > id,
        "id 為資料庫自增且不重用"
    );
}

#[tokio::test]
async fn create_requires_description_and_location() {
    let pool = test_pool().await;

    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/v1/assets",
        Some(json!({ "brand": "Dell" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "validation_error");
    assert_eq!(body["message"], "描述為必填");
    assert_eq!(body["details"]["field"], "description");

    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/v1/assets",
        Some(json!({ "description": "   " })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["message"], "描述為必填", "全空白視為缺漏");

    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/v1/assets",
        Some(json!({ "description": "電腦" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["message"], "位置為必填");
    assert_eq!(body["details"]["field"], "location");

    // 選填欄位的結構錯誤也必須擋下（見 ADR-0006）
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/v1/assets",
        Some(json!({
            "description": "電腦",
            "location": "機房",
            "purchase_date": "2020/01/01"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["details"]["field"], "purchase_date");

    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/v1/assets",
        Some(json!({
            "description": "電腦",
            "location": "機房",
            "lifespan_years": -1
        })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["details"]["field"], "lifespan_years");

    // 編輯時的必填驗證同樣阻擋
    let asset = create_asset(&pool, json!({ "description": "電腦", "location": "機房" })).await;
    let id = asset["id"].as_i64().expect("回應含 id");
    let (status, body) = send(
        &pool,
        Method::PATCH,
        &format!("/api/v1/assets/{id}"),
        Some(json!({ "location": "" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["message"], "位置為必填");

    // 驗證失敗不寫入任何資料
    let (_, page) = send(&pool, Method::GET, "/api/v1/assets", None).await;
    assert_eq!(page["total"], 1, "僅存在一筆成功建立的資產");
}

#[tokio::test]
async fn list_searches_and_filters_server_side() {
    let pool = test_pool().await;

    create_asset(
        &pool,
        json!({
            "property_no": "PC-001",
            "description": "會計部電腦",
            "location": "總公司 3F",
            "device_serial": "SN-AAA",
            "brand": "Lenovo",
            "model": "ThinkCentre",
            "note": "財務用"
        }),
    )
    .await;
    create_asset(
        &pool,
        json!({
            "description": "機房交換器",
            "location": "機房 A",
            "device_serial": "SW-XYZ",
            "brand": "Cisco",
            "model": "C9300",
            "note": "核心交換器"
        }),
    )
    .await;
    create_asset(
        &pool,
        json!({
            "description": "櫃檯印表機",
            "location": "總公司 3F",
            "brand": "HP",
            "model": "LaserJet",
            "note": "彩色"
        }),
    )
    .await;

    // 關鍵字跨 財產編號／描述／設備序號／廠牌／型號／備註；子字串、不分大小寫
    let cases = [
        ("PC-001", "會計部電腦"),
        ("會計", "會計部電腦"),
        ("SN-AAA", "會計部電腦"),
        ("thinkcentre", "會計部電腦"),
        ("財務", "會計部電腦"),
        ("c9300", "機房交換器"),
        ("彩色", "櫃檯印表機"),
    ];
    for (q, expected) in cases {
        let (status, page) = send(&pool, Method::GET, &format!("/api/v1/assets?q={q}"), None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(page["total"], 1, "q={q} 應命中 1 筆");
        assert_eq!(page["items"][0]["description"], expected, "q={q}");
    }

    // 關鍵字含 LIKE 萬用字元時視為字面字元
    let (_, page) = send(&pool, Method::GET, "/api/v1/assets?q=%25", None).await;
    assert_eq!(page["total"], 0, "% 不應變成萬用字元");

    // 位置與廠牌篩選：不分大小寫完全符合
    let (status, page) = send(
        &pool,
        Method::GET,
        &format!("/api/v1/assets?location={}", encode("總公司 3F")),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(page["total"], 2);

    let (_, page) = send(
        &pool,
        Method::GET,
        &format!("/api/v1/assets?location={}", encode("機房 a")),
        None,
    )
    .await;
    assert_eq!(page["total"], 1, "位置篩選不分大小寫");
    assert_eq!(page["items"][0]["description"], "機房交換器");

    let (_, page) = send(&pool, Method::GET, "/api/v1/assets?brand=hp", None).await;
    assert_eq!(page["total"], 1);
    assert_eq!(page["items"][0]["description"], "櫃檯印表機");

    // 關鍵字＋篩選可組合
    let (_, page) = send(
        &pool,
        Method::GET,
        &format!(
            "/api/v1/assets?q={}&location={}",
            encode("電腦"),
            encode("總公司 3F")
        ),
        None,
    )
    .await;
    assert_eq!(page["total"], 1);
    assert_eq!(page["items"][0]["description"], "會計部電腦");
}

#[tokio::test]
async fn list_paginates_and_orders_by_description() {
    let pool = test_pool().await;
    for description in ["Bravo", "alpha", "Charlie"] {
        create_asset(
            &pool,
            json!({ "description": description, "location": "機房" }),
        )
        .await;
    }

    let (status, page) = send(&pool, Method::GET, "/api/v1/assets?per_page=2&page=1", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(page["total"], 3);
    assert_eq!(page["page"], 1);
    assert_eq!(page["per_page"], 2);
    assert_eq!(page["items"].as_array().expect("items 為陣列").len(), 2);

    // 預設排序：描述升冪（不分大小寫）
    assert_eq!(page["items"][0]["description"], "alpha");
    assert_eq!(page["items"][1]["description"], "Bravo");

    let (_, page) = send(&pool, Method::GET, "/api/v1/assets?per_page=2&page=2", None).await;
    assert_eq!(page["items"].as_array().expect("items 為陣列").len(), 1);
    assert_eq!(page["items"][0]["description"], "Charlie");

    // 預設每頁 50 筆（spec §6）
    let (_, page) = send(&pool, Method::GET, "/api/v1/assets", None).await;
    assert_eq!(page["per_page"], 50);
}

#[tokio::test]
async fn locations_and_brands_are_deduplicated_case_insensitively() {
    let pool = test_pool().await;

    for (description, location, brand) in [
        ("一", "機房 A", "Dell"),
        ("二", "MDF", "HP"),
        ("三", "mdf", "hp"),
        ("四", " 機房 A ", "DELL"),
    ] {
        create_asset(
            &pool,
            json!({ "description": description, "location": location, "brand": brand }),
        )
        .await;
    }

    let (status, body) = send(&pool, Method::GET, "/api/v1/locations", None).await;
    assert_eq!(status, StatusCode::OK);
    let locations: Vec<&str> = body["items"]
        .as_array()
        .expect("items 為陣列")
        .iter()
        .map(|value| value.as_str().expect("位置為字串"))
        .collect();
    assert_eq!(
        locations,
        vec!["MDF", "機房 A"],
        "不分大小寫去重，保留最早寫入的原文"
    );

    let (_, body) = send(&pool, Method::GET, "/api/v1/brands", None).await;
    let brands: Vec<&str> = body["items"]
        .as_array()
        .expect("items 為陣列")
        .iter()
        .map(|value| value.as_str().expect("廠牌為字串"))
        .collect();
    assert_eq!(brands, vec!["Dell", "HP"]);
}

#[tokio::test]
async fn duplicate_device_serial_is_only_a_hint() {
    let pool = test_pool().await;

    create_asset(
        &pool,
        json!({ "description": "設備一", "location": "機房", "device_serial": "SN-001" }),
    )
    .await;
    let second = create_asset(
        &pool,
        json!({ "description": "設備二", "location": "機房", "device_serial": "sn-001" }),
    )
    .await;
    assert!(
        second["id"].as_i64().is_some_and(|id| id > 0),
        "重複設備序號不阻擋建立"
    );

    // 精確查找（前端據此顯示提示，不阻擋）
    let (_, page) = send(
        &pool,
        Method::GET,
        "/api/v1/assets?device_serial=SN-001",
        None,
    )
    .await;
    assert_eq!(page["total"], 2, "不分大小寫的精確查找");
}

#[tokio::test]
async fn expired_flag_is_computed_by_server() {
    use chrono::{Duration, Local};

    let pool = test_pool().await;
    let today = Local::now().date_naive();
    let format_date = |date: chrono::NaiveDate| date.format("%Y-%m-%d").to_string();

    let cases = [
        // （購置日期, 年限, 期望屆齡）
        (Some(today - Duration::days(4000)), Some(10), true),
        (Some(today), Some(0), false),
        (Some(today - Duration::days(1)), Some(0), true),
        (Some(today), None, false),
        (None, Some(10), false),
    ];

    for (index, (purchase_date, lifespan_years, expected)) in cases.into_iter().enumerate() {
        let body = json!({
            "description": format!("資產 {index}"),
            "location": "機房",
            "purchase_date": purchase_date.map(format_date),
            "lifespan_years": lifespan_years,
        });
        let created = create_asset(&pool, body).await;
        assert_eq!(
            created["expired"], expected,
            "purchase_date={:?}, lifespan_years={:?}",
            purchase_date, lifespan_years
        );
    }

    // 屆齡僅提示，不影響任何操作
    let (_, page) = send(&pool, Method::GET, "/api/v1/assets", None).await;
    let expired_id = page["items"]
        .as_array()
        .expect("items 為陣列")
        .iter()
        .find(|item| item["expired"] == true)
        .and_then(|item| item["id"].as_i64())
        .expect("清單中有屆齡資產");

    let (status, updated) = send(
        &pool,
        Method::PATCH,
        &format!("/api/v1/assets/{expired_id}"),
        Some(json!({ "note": "屆齡仍可編輯" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["note"], "屆齡仍可編輯");

    let (status, _) = send(
        &pool,
        Method::DELETE,
        &format!("/api/v1/assets/{expired_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "屆齡仍可刪除");
}
