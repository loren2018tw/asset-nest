//! 清冊稽核整合測試：`.xlsx`／`.xls` 解析、多檔合併、兩向差異與原文保留、
//! 檔級錯誤整批拒絕、唯讀（見票 01）。

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use sqlx::SqlitePool;
use sqlx::sqlite::SqlitePoolOptions;
use tower::ServiceExt;

use asset_nest::{AppState, app};

/// 合成 fixtures（由 `tests/fixtures/make_fixtures.py` 產出；不含真實資料）。
const PROPERTY_XLSX: &[u8] = include_bytes!("fixtures/property.xlsx");
const PROPERTY_XLS: &[u8] = include_bytes!("fixtures/property.xls");
const ITEMS_XLSX: &[u8] = include_bytes!("fixtures/items.xlsx");
const HEADER_ONLY_XLSX: &[u8] = include_bytes!("fixtures/header_only.xlsx");
const NO_HEADER_XLSX: &[u8] = include_bytes!("fixtures/no_header.xlsx");
const MANY_ROWS_XLSX: &[u8] = include_bytes!("fixtures/many_rows.xlsx");
const MAX_ROWS_XLSX: &[u8] = include_bytes!("fixtures/max_rows.xlsx");

/// 建立測試資料庫並套用 migrations（比照 `tests/import.rs`）。
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

/// 以 `oneshot` 發送 JSON 請求；回傳狀態碼與 JSON。
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

    execute(pool, builder.body(body).expect("建立請求")).await
}

/// 以 `oneshot` 發送原始 bytes 請求（multipart 測試用）。
async fn send_raw(
    pool: &SqlitePool,
    method: Method,
    uri: &str,
    content_type: &str,
    body: Vec<u8>,
) -> (StatusCode, Value) {
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::CONTENT_TYPE, content_type)
        .body(Body::from(body))
        .expect("建立請求");

    execute(pool, request).await
}

async fn execute(pool: &SqlitePool, request: Request<Body>) -> (StatusCode, Value) {
    let response = app(AppState::new(
        pool.clone(),
        std::env::temp_dir().join("asset-nest-test-no-dist"),
    ))
    .oneshot(request)
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

const BOUNDARY: &str = "asset-nest-inventory-audit-boundary";

/// 以 multipart（同名檔案欄位 `file`）呼叫清冊稽核端點。
async fn post_audit(pool: &SqlitePool, files: &[(&str, &[u8])]) -> (StatusCode, Value) {
    let mut body = Vec::new();
    for (filename, bytes) in files {
        body.extend_from_slice(
            format!(
                "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"file\"; \
                 filename=\"{filename}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
            )
            .as_bytes(),
        );
        body.extend_from_slice(bytes);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{BOUNDARY}--\r\n").as_bytes());

    send_raw(
        pool,
        Method::POST,
        "/api/v1/inventory-audit",
        &format!("multipart/form-data; boundary={BOUNDARY}"),
        body,
    )
    .await
}

/// 新增資產並斷言成功，回傳 id。
async fn create_asset(pool: &SqlitePool, body: Value) -> i64 {
    let (status, json) = send(pool, Method::POST, "/api/v1/assets", Some(body)).await;
    assert_eq!(status, StatusCode::CREATED, "新增資產應成功：{json}");
    json["id"].as_i64().expect("回應含 id")
}

/// 讀取 `assets` 全列作為唯讀比對基準。
async fn asset_rows(
    pool: &SqlitePool,
) -> Vec<(i64, Option<String>, String, String, Option<String>)> {
    sqlx::query_as("SELECT id, property_no, description, location, note FROM assets ORDER BY id")
        .fetch_all(pool)
        .await
        .expect("讀取資產")
}

/// 依正規化比對鍵尋找差異列（測試輔助）。
fn row_with_key<'a>(rows: &'a Value, property_no: &str) -> &'a Value {
    rows.as_array()
        .expect("差異表為陣列")
        .iter()
        .find(|row| row["property_no"] == json!(property_no))
        .unwrap_or_else(|| panic!("找不到財產編號 {property_no:?}"))
}

#[tokio::test]
async fn xlsx_audit_reports_both_directions_and_preserves_original_text() {
    let pool = test_pool().await;
    create_asset(
        &pool,
        json!({
            "description": "伺服器主機",
            "location": "電腦機房",
            "property_no": "3140101-03\n\n3000380",
        }),
    )
    .await;
    let missing_1 = create_asset(
        &pool,
        json!({
            "description": "舊印表機",
            "location": "倉庫",
            "property_no": "3000874",
            "note": "166823",
        }),
    )
    .await;
    let missing_2 = create_asset(
        &pool,
        json!({ "description": "重複甲", "location": "機房", "property_no": "DUP-1" }),
    )
    .await;
    let missing_3 = create_asset(
        &pool,
        json!({ "description": "重複乙", "location": "機房", "property_no": "dup-1" }),
    )
    .await;
    // 無財產編號的資產不進系統側。
    create_asset(
        &pool,
        json!({ "description": "無編號設備", "location": "倉庫" }),
    )
    .await;

    let (status, body) = post_audit(&pool, &[("property.xlsx", PROPERTY_XLSX)]).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    assert_eq!(
        body["files"],
        json!([{ "filename": "property.xlsx", "kind": "property", "total": 3, "skipped": 1 }])
    );
    assert_eq!(
        body["summary"],
        json!({
            "list_total": 3, "list_property": 3, "list_item": 0,
            "system_total": 4, "matched": 1,
            "missing_in_system": 2, "missing_in_list": 3,
        })
    );

    // 清冊有、系統沒有：同鍵（大小寫與空白差異）兩列逐列呈現、皆標重複。
    let rows = &body["missing_in_system"];
    assert_eq!(rows.as_array().expect("陣列").len(), 2);
    let row = row_with_key(rows, "PC-100");
    assert_eq!(row["file"], "property.xlsx");
    assert_eq!(row["kind"], "property");
    assert_eq!(row["row_number"], 5);
    assert_eq!(row["name"], "個人電腦");
    assert_eq!(row["form"], "桌上型");
    assert_eq!(row["location"], "辦公室A");
    assert_eq!(row["custodian"], "李四");
    assert_eq!(row["note"], "備註甲");
    assert_eq!(row["duplicate"], true);
    let row = row_with_key(rows, " pc-100\n");
    assert_eq!(row["row_number"], 6);
    assert_eq!(row["duplicate"], true);
    assert!(row["note"].is_null(), "空儲存格＝null");

    // 系統有、清冊沒有：依鍵升冪；系統側同鍵兩列皆標重複；原文保留。
    let rows = &body["missing_in_list"];
    assert_eq!(rows.as_array().expect("陣列").len(), 3);
    assert_eq!(rows[0]["asset_id"], missing_1);
    assert_eq!(rows[0]["property_no"], "3000874");
    assert_eq!(rows[0]["description"], "舊印表機");
    assert_eq!(rows[0]["location"], "倉庫");
    assert_eq!(rows[0]["note"], "166823");
    assert_eq!(rows[0]["duplicate"], false);
    assert_eq!(rows[1]["asset_id"], missing_2);
    assert_eq!(rows[1]["property_no"], "DUP-1");
    assert_eq!(rows[1]["duplicate"], true);
    assert_eq!(rows[2]["asset_id"], missing_3);
    assert_eq!(rows[2]["property_no"], "dup-1");
    assert_eq!(rows[2]["duplicate"], true);

    // 多行編號（含空行）之原樣比對：matched＝1，兩側同鍵不再出現在差異表。
    assert!(
        body["missing_in_system"]
            .as_array()
            .expect("陣列")
            .iter()
            .all(|row| row["property_no"] != json!("3140101-03\n\n3000380")),
        "命中的多行編號不應出現於差異表：{body}"
    );
}

#[tokio::test]
async fn xls_audit_preserves_multiline_property_numbers() {
    let pool = test_pool().await;
    // 系統側刻意留空：三列全部落在「清冊有、系統沒有」，逐列驗證 xls（BIFF）
    // 直讀後的原文（多行編號、空行、空白）完整保留。
    let (status, body) = post_audit(&pool, &[("property.xls", PROPERTY_XLS)]).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    assert_eq!(
        body["files"],
        json!([{ "filename": "property.xls", "kind": "property", "total": 3, "skipped": 1 }])
    );
    assert_eq!(body["summary"]["matched"], 0);
    assert_eq!(body["summary"]["system_total"], 0);
    assert_eq!(body["summary"]["missing_in_system"], 3);
    assert_eq!(body["summary"]["missing_in_list"], 0);

    let row = row_with_key(&body["missing_in_system"], "3140101-03\n\n3000380");
    assert_eq!(row["row_number"], 4);
    assert_eq!(row["name"], "伺服器主機");
    assert_eq!(row["form"], "機架式\n2U", "形式品名多行亦保留");
    assert_eq!(row["location"], "電腦機房");
    assert_eq!(row["custodian"], "王小明");
    assert!(row["note"].is_null());
    assert_eq!(row["duplicate"], false);

    let row = row_with_key(&body["missing_in_system"], "PC-100");
    assert_eq!(row["row_number"], 5);
    assert_eq!(row["note"], "備註甲");
    let row = row_with_key(&body["missing_in_system"], " pc-100\n");
    assert_eq!(row["row_number"], 6);
    assert_eq!(row["duplicate"], true);
}

#[tokio::test]
async fn multi_file_audit_merges_lists_and_uses_per_file_kind() {
    let pool = test_pool().await;
    create_asset(
        &pool,
        json!({
            "description": "伺服器主機",
            "location": "電腦機房",
            "property_no": "3140101-03\n\n3000380",
        }),
    )
    .await;
    create_asset(
        &pool,
        json!({
            "description": "雷射印表機",
            "location": "辦公室B",
            "property_no": "60102 6005129",
        }),
    )
    .await;

    let (status, body) = post_audit(
        &pool,
        &[
            ("資訊組財產清冊.xlsx", PROPERTY_XLSX),
            ("items.xlsx", ITEMS_XLSX),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    assert_eq!(
        body["files"],
        json!([
            { "filename": "資訊組財產清冊.xlsx", "kind": "property", "total": 3, "skipped": 1 },
            { "filename": "items.xlsx", "kind": "item", "total": 2, "skipped": 0 },
        ])
    );
    assert_eq!(
        body["summary"],
        json!({
            "list_total": 5, "list_property": 3, "list_item": 2,
            "system_total": 2, "matched": 2,
            "missing_in_system": 3, "missing_in_list": 0,
        })
    );

    // 跨檔合併後依鍵升冪：pc-100（財產）在前、x3009999（物品）在後。
    let rows = &body["missing_in_system"];
    assert_eq!(
        rows.as_array()
            .expect("陣列")
            .iter()
            .map(|row| row["property_no"].clone())
            .collect::<Vec<_>>(),
        vec![
            json!("PC-100"),
            json!(" pc-100\n"),
            json!("\u{3000}x3009999\u{3000}")
        ]
    );
    let row = row_with_key(rows, "\u{3000}x3009999\u{3000}");
    assert_eq!(row["file"], "items.xlsx");
    assert_eq!(row["kind"], "item");
    assert_eq!(row["row_number"], 4);
    assert_eq!(row["duplicate"], false);
}

#[tokio::test]
async fn file_level_errors_reject_the_whole_audit() {
    let pool = test_pool().await;
    let before = asset_rows(&pool).await;

    // 非 .xls／.xlsx 副檔名。
    let (status, body) = post_audit(
        &pool,
        &[(
            "report.csv",
            b"\xe8\xb2\xa1\xe7\x94\xa2\xe7\xb7\xa8\xe8\x99\x9f",
        )],
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], "validation_error");
    assert_eq!(body["details"]["field"], "file");
    let files = body["details"]["files"].as_array().expect("files 陣列");
    assert_eq!(files.len(), 1);
    assert_eq!(files[0]["filename"], "report.csv");
    assert!(
        files[0]["message"]
            .as_str()
            .expect("訊息為字串")
            .contains(".xls"),
        "{body}"
    );

    // 損毀的 .xlsx。
    let (status, body) = post_audit(&pool, &[("broken.xlsx", b"not an excel")]).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(
        body["message"].as_str().expect("訊息").contains("無法解析"),
        "{body}"
    );

    // 找不到「財產編號」表頭。
    let (status, body) = post_audit(&pool, &[("no_header.xlsx", NO_HEADER_XLSX)]).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(
        body["message"].as_str().expect("訊息").contains("財產編號"),
        "{body}"
    );

    // 整檔無資料列（僅表頭）。
    let (status, body) = post_audit(&pool, &[("header_only.xlsx", HEADER_ONLY_XLSX)]).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(
        body["message"]
            .as_str()
            .expect("訊息")
            .contains("沒有資料列"),
        "{body}"
    );

    // 單檔資料列超過 5,000 列。
    let (status, body) = post_audit(&pool, &[("many_rows.xlsx", MANY_ROWS_XLSX)]).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(
        body["message"].as_str().expect("訊息").contains("5,000"),
        "{body}"
    );

    // 單檔超過 5 MB（讀取時截斷，以 oversized 標記整批拒絕）。
    let oversized = vec![0u8; 5 * 1024 * 1024 + 1];
    let (status, body) = post_audit(&pool, &[("big.xlsx", &oversized)]).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(
        body["message"].as_str().expect("訊息").contains("5 MB"),
        "{body}"
    );

    // 一檔壞即整批拒絕：正常檔＋壞檔同送亦 400，`files` 僅列壞檔。
    let (status, body) = post_audit(
        &pool,
        &[
            ("property.xlsx", PROPERTY_XLSX),
            ("no_header.xlsx", NO_HEADER_XLSX),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    let files = body["details"]["files"].as_array().expect("files 陣列");
    assert_eq!(files.len(), 1, "僅列出錯的檔案");
    assert_eq!(files[0]["filename"], "no_header.xlsx");

    // 唯讀：所有錯誤案例均未異動 assets。
    assert_eq!(asset_rows(&pool).await, before);
}

#[tokio::test]
async fn more_than_ten_files_are_rejected() {
    let pool = test_pool().await;
    let files: Vec<(&str, &[u8])> = (0..11).map(|_| ("property.xlsx", PROPERTY_XLSX)).collect();

    let (status, body) = post_audit(&pool, &files).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], "validation_error");
    assert_eq!(body["details"]["field"], "file");
    assert!(
        body["message"].as_str().expect("訊息").contains("10"),
        "{body}"
    );
    let entries = body["details"]["files"].as_array().expect("files 陣列");
    assert_eq!(entries.len(), 11, "逐檔列出已收到的檔案");
}

#[tokio::test]
async fn exactly_five_thousand_rows_is_accepted() {
    let pool = test_pool().await;

    let (status, body) = post_audit(&pool, &[("max_rows.xlsx", MAX_ROWS_XLSX)]).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["files"][0]["total"], 5000);
    assert_eq!(body["summary"]["list_total"], 5000);
    assert_eq!(
        body["missing_in_system"].as_array().expect("陣列").len(),
        5000
    );
}

#[tokio::test]
async fn audit_is_read_only_for_assets() {
    let pool = test_pool().await;
    create_asset(
        &pool,
        json!({ "description": "甲", "location": "機房", "property_no": "3000874" }),
    )
    .await;
    create_asset(&pool, json!({ "description": "乙", "location": "機房" })).await;
    let before = asset_rows(&pool).await;

    let (status, _) = post_audit(&pool, &[("property.xlsx", PROPERTY_XLSX)]).await;
    assert_eq!(status, StatusCode::OK);

    assert_eq!(asset_rows(&pool).await, before, "稽核前後 assets 不變");
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM assets")
        .fetch_one(&pool)
        .await
        .expect("查詢筆數");
    assert_eq!(count, 2);
}

#[tokio::test]
async fn missing_file_field_and_non_multipart_return_400() {
    let pool = test_pool().await;

    // 沒有 file 欄位。
    let body =
        format!("--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"other\"\r\n\r\nx\r\n--{BOUNDARY}--\r\n")
            .into_bytes();
    let (status, body) = send_raw(
        &pool,
        Method::POST,
        "/api/v1/inventory-audit",
        &format!("multipart/form-data; boundary={BOUNDARY}"),
        body,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(
        body["message"].as_str().expect("訊息").contains("file"),
        "{body}"
    );

    // 非 multipart 請求。
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/v1/inventory-audit",
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(
        body["message"]
            .as_str()
            .expect("訊息")
            .contains("multipart"),
        "{body}"
    );
}
