//! 網段 CSV 匯入整合測試：編碼、標題、7 欄規則（含排除範圍）、跨列衝突、
//! dry_run 預覽與單一交易寫入（見票 02、05）。

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

/// 以 `oneshot` 發送 JSON 請求；回傳狀態碼與 JSON（204 等空內容為 `Value::Null`）。
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

/// 以 `oneshot` 發送 GET；回傳狀態碼與原始內容（CSV 回應用）。
async fn send_bytes(pool: &SqlitePool, uri: &str) -> (StatusCode, Vec<u8>) {
    let request = Request::builder()
        .method(Method::GET)
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
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("讀取回應內容")
        .to_bytes()
        .to_vec();

    (status, bytes)
}

const BOUNDARY: &str = "asset-nest-subnet-import-boundary";

/// 以 multipart（檔案欄位 `file`）呼叫匯入端點；`dry_run` 為 `None` 時不帶查詢參數。
async fn post_csv(pool: &SqlitePool, dry_run: Option<bool>, bytes: &[u8]) -> (StatusCode, Value) {
    let uri = match dry_run {
        Some(value) => format!("/api/v1/subnets/import?dry_run={value}"),
        None => "/api/v1/subnets/import".to_string(),
    };

    let mut body = Vec::new();
    body.extend_from_slice(
        format!(
            "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"file\"; \
             filename=\"subnets.csv\"\r\nContent-Type: text/csv\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(bytes);
    body.extend_from_slice(format!("\r\n--{BOUNDARY}--\r\n").as_bytes());

    send_raw(
        pool,
        Method::POST,
        &uri,
        &format!("multipart/form-data; boundary={BOUNDARY}"),
        body,
    )
    .await
}

/// 匯入 CSV 標題列（7 欄，順序同 ADR-0009／0020）。
const HEADERS: &str = "名稱,CIDR,Gateway,Kea subnet-id,位址池,排除範圍,備註";

/// 7 欄資料列；預設僅填 CIDR，其餘留空。
#[derive(Default)]
struct CsvRow {
    name: String,
    cidr: String,
    gateway: String,
    kea_subnet_id: String,
    pools: String,
    exclusions: String,
    note: String,
}

impl CsvRow {
    fn new(cidr: &str) -> Self {
        Self {
            cidr: cidr.to_string(),
            ..Self::default()
        }
    }

    fn fields(&self) -> [&str; 7] {
        [
            &self.name,
            &self.cidr,
            &self.gateway,
            &self.kea_subnet_id,
            &self.pools,
            &self.exclusions,
            &self.note,
        ]
    }
}

/// 以標準 7 欄標題組出 CSV 文字。
fn build_csv(rows: &[CsvRow]) -> String {
    let mut text = String::from(HEADERS);
    for row in rows {
        text.push('\n');
        text.push_str(&row.fields().join(","));
    }
    text.push('\n');
    text
}

/// 以 API 新增既有網段並斷言成功，回傳回應 JSON。
async fn create_subnet(pool: &SqlitePool, body: Value) -> Value {
    let (status, json) = send(pool, Method::POST, "/api/v1/subnets", Some(body)).await;
    assert_eq!(status, StatusCode::CREATED, "新增網段應成功：{json}");
    json
}

/// 以 SQL 讀取純量（測試以直接查 DB 驗證落地與零殘留）。
async fn count(pool: &SqlitePool, sql: &str) -> i64 {
    sqlx::query_scalar(sql)
        .fetch_one(pool)
        .await
        .expect("查詢資料庫")
}

/// 取得回應中指定列號的報告列。
fn row_at(report: &Value, row_number: usize) -> &Value {
    report["rows"]
        .as_array()
        .expect("rows 為陣列")
        .iter()
        .find(|row| row["row_number"] == json!(row_number))
        .unwrap_or_else(|| panic!("找不到第 {row_number} 列報告"))
}

/// 該列是否含指定問題代碼。
fn has_code(row: &Value, code: &str) -> bool {
    row["issues"]
        .as_array()
        .expect("issues 為陣列")
        .iter()
        .any(|issue| issue["code"] == code)
}

/// 該列指定問題的第一個訊息。
fn message_of(row: &Value, code: &str) -> String {
    row["issues"]
        .as_array()
        .expect("issues 為陣列")
        .iter()
        .find(|issue| issue["code"] == code)
        .and_then(|issue| issue["message"].as_str())
        .unwrap_or_else(|| panic!("找不到問題代碼 {code}"))
        .to_string()
}

#[tokio::test]
async fn import_route_is_static_and_defaults_to_dry_run() {
    let pool = test_pool().await;

    // 靜態路徑 `/subnets/import` 須命中匯入端點，而非 `/subnets/{id}`。
    let csv = build_csv(&[CsvRow::new("10.0.0.0/24")]);
    let (status, report) = post_csv(&pool, None, csv.as_bytes()).await;
    assert_eq!(status, StatusCode::OK, "匯入端點應回 200：{report}");
    assert_eq!(report["dry_run"], true, "dry_run 預設 true");
    assert_eq!(report["summary"]["total"], 1);
    assert_eq!(report["summary"]["ok"], 1);
    assert_eq!(report["summary"]["warnings"], 0, "網段匯入無語意警示");
    assert_eq!(report["committed"], false);
    assert!(report["created"].is_null());

    // 非 multipart 請求也進到匯入端點（動態路由無 POST，會是 405）。
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/v1/subnets/import",
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "validation_error");
    assert!(
        body["message"]
            .as_str()
            .expect("訊息為字串")
            .contains("multipart"),
        "應提示 multipart：{body}"
    );

    // dry_run 不寫任何資料。
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM subnets").await, 0);
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM subnet_pools").await, 0);

    // 缺少 file 欄位 → 400。
    let body = format!(
        "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"other\"\r\n\r\nx\r\n--{BOUNDARY}--\r\n"
    )
    .into_bytes();
    let (status, body) = send_raw(
        &pool,
        Method::POST,
        "/api/v1/subnets/import",
        &format!("multipart/form-data; boundary={BOUNDARY}"),
        body,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(
        body["message"]
            .as_str()
            .expect("訊息為字串")
            .contains("file"),
        "{body}"
    );

    // dry_run 值無效 → 400。
    let (status, body) = send_raw(
        &pool,
        Method::POST,
        "/api/v1/subnets/import?dry_run=maybe",
        &format!("multipart/form-data; boundary={BOUNDARY}"),
        Vec::new(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[tokio::test]
async fn headers_are_trimmed_case_insensitive_and_order_free() {
    let pool = test_pool().await;

    // 未知標題忽略、英文不分大小寫、順序不拘、單元格 trim、CIDR 正規化；
    // 「排除範圍」欄缺席視為未填、出現時解析段與用途說明。
    let text = " 名稱 ,排除範圍,數量, CIDR , gateway ,KEA SUBNET-ID,位址池,備註\n 核心 , 10.0.0.30-10.0.0.40#NAT 對外 ,3, 10.0.0.5/24 , 10.0.0.1 , 7 , 10.0.0.100-10.0.0.150 , 主要 \n";
    let (status, report) = post_csv(&pool, Some(true), text.as_bytes()).await;
    assert_eq!(status, StatusCode::OK, "{report}");
    assert_eq!(report["ignored_headers"], json!(["數量"]));
    assert_eq!(report["summary"]["errors"], 0);

    let row = row_at(&report, 2);
    assert_eq!(row["status"], "ok");
    assert_eq!(row["data"]["name"], "核心");
    assert_eq!(row["data"]["cidr"], "10.0.0.0/24", "host bits 收斂");
    assert_eq!(row["data"]["gateway"], "10.0.0.1");
    assert_eq!(row["data"]["kea_subnet_id"], 7);
    assert_eq!(row["data"]["pools"], json!(["10.0.0.100-10.0.0.150"]));
    assert_eq!(
        row["data"]["exclusions"],
        json!(["10.0.0.30-10.0.0.40#NAT 對外"]),
        "7 欄標題順序不拘；用途說明 trim"
    );
    assert_eq!(row["data"]["note"], "主要");
}

#[tokio::test]
async fn missing_or_duplicate_headers_return_400() {
    let pool = test_pool().await;

    // 缺 CIDR 標題。
    let (status, body) = post_csv(
        &pool,
        Some(true),
        "名稱,Gateway\n核心,10.0.0.1\n".as_bytes(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], "validation_error");
    let message = body["message"].as_str().expect("訊息為字串");
    assert!(message.contains("缺少必要標題"), "{message}");
    assert!(message.contains("CIDR"), "{message}");
    assert_eq!(body["details"]["field"], "file");

    // 重複標題。
    let (status, body) = post_csv(
        &pool,
        Some(true),
        "CIDR,CIDR,名稱\n10.0.0.0/24,10.0.1.0/24,核心\n".as_bytes(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(
        body["message"].as_str().expect("訊息").contains("標題重複"),
        "{body}"
    );

    // 未知標題重複同樣視為標題重複。
    let (status, body) = post_csv(
        &pool,
        Some(true),
        "CIDR,名稱,數量,數量\n10.0.0.0/24,核心,1,2\n".as_bytes(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(
        body["message"].as_str().expect("訊息").contains("數量"),
        "{body}"
    );
}

#[tokio::test]
async fn detects_utf8_bom_and_big5() {
    let pool = test_pool().await;

    // UTF-8 BOM：BOM 去除、encoding 為 utf-8。
    let csv = build_csv(&[CsvRow::new("10.0.0.0/24")]);
    let mut with_bom = vec![0xEF, 0xBB, 0xBF];
    with_bom.extend_from_slice(csv.as_bytes());
    let (status, report) = post_csv(&pool, Some(true), &with_bom).await;
    assert_eq!(status, StatusCode::OK, "{report}");
    assert_eq!(report["encoding"], "utf-8");
    assert_eq!(row_at(&report, 2)["data"]["cidr"], "10.0.0.0/24");

    // Big5：以 bytes 造檔並偵測。
    let source = "名稱,CIDR\n核心,10.0.0.0/24\n";
    let (big5, _, _) = encoding_rs::BIG5.encode(source);
    let (status, report) = post_csv(&pool, Some(true), &big5).await;
    assert_eq!(status, StatusCode::OK, "{report}");
    assert_eq!(report["encoding"], "big5");
    assert_eq!(report["summary"]["ok"], 1);
    assert_eq!(row_at(&report, 2)["data"]["name"], "核心");

    // UTF-16 BOM 與無法解讀的 bytes 皆回 400 並提示另存。
    for broken in [
        vec![0xFF, 0xFE, 0x41, 0x00],
        vec![0xFF, 0xFF, 0xFF, 0xFF],
        vec![0xC0, 0x80, 0xC0, 0x80],
    ] {
        let (status, body) = post_csv(&pool, Some(true), &broken).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        let message = body["message"].as_str().expect("訊息");
        assert!(
            message.contains("UTF-8") && message.contains("Big5"),
            "應提示另存 UTF-8／Big5：{message}"
        );
    }
}

#[tokio::test]
async fn rejects_empty_files_rows_limits_and_size() {
    let pool = test_pool().await;

    let (status, body) = post_csv(&pool, Some(true), b"").await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");

    // 僅標題列＝無資料列。
    let (status, body) = post_csv(&pool, Some(true), format!("{HEADERS}\n").as_bytes()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body["message"]
            .as_str()
            .expect("訊息")
            .contains("沒有資料列"),
        "{body}"
    );

    // 超過 5,000 列（解析即擋下，不進入列級驗證）。
    let mut text = String::from("CIDR\n");
    for index in 0..5_001 {
        text.push_str(&format!("10.{}.{}.0/24\n", index / 256, index % 256));
    }
    let (status, body) = post_csv(&pool, Some(true), text.as_bytes()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(
        body["message"].as_str().expect("訊息").contains("5,000"),
        "{body}"
    );

    // 超過 5 MB：由匯入端點自行回 400（不被 axum body limit 擋成 413）。
    let oversized = vec![b'a'; 5 * 1024 * 1024 + 1];
    let (status, body) = post_csv(&pool, Some(true), &oversized).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(
        body["message"].as_str().expect("訊息").contains("5 MB"),
        "{body}"
    );
}

#[tokio::test]
async fn column_count_mismatch_is_a_row_error() {
    let pool = test_pool().await;

    let mut csv = String::from(HEADERS);
    csv.push_str("\n10.0.0.0/24\n"); // 欄位過少
    csv.push_str("10.0.1.0/24,網段,10.0.1.1,1,10.0.1.10-10.0.1.20,,備註,多餘\n"); // 欄位過多

    let (status, report) = post_csv(&pool, Some(false), csv.as_bytes()).await;
    assert_eq!(status, StatusCode::OK, "{report}");
    assert_eq!(report["summary"]["errors"], 2);
    assert_eq!(report["committed"], false);
    assert!(report["created"].is_null());

    let row = row_at(&report, 2);
    assert_eq!(row["status"], "error");
    assert!(has_code(row, "column_count_mismatch"));
    assert!(message_of(row, "column_count_mismatch").contains("7"));
    assert_eq!(
        row["data"]["exclusions"],
        json!([]),
        "mismatch_data 一併填入排除範圍"
    );

    // 整批不匯入，無殘留。
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM subnets").await, 0);
}

#[tokio::test]
async fn validates_cidr_rules_and_normalizes() {
    let pool = test_pool().await;
    create_subnet(
        &pool,
        json!({ "cidr": "10.10.0.0/16", "name": "既有大網段" }),
    )
    .await;
    create_subnet(&pool, json!({ "cidr": "192.168.0.0/24" })).await;

    let ok = CsvRow::new("10.0.0.0/24"); // 列 2
    let normalized = CsvRow::new("172.16.5.5/24"); // 列 3
    let missing = CsvRow {
        name: "缺 CIDR".to_string(),
        ..CsvRow::default()
    }; // 列 4
    let invalid = CsvRow::new("10.0.0"); // 列 5
    let duplicate_existing = CsvRow::new("10.10.0.0/16"); // 列 6
    let overlap_existing = CsvRow::new("10.10.1.0/24"); // 列 7
    let duplicate_file = CsvRow::new("10.0.0.1/24"); // 列 8（與列 2 正規化後相同）
    let overlap_file = CsvRow::new("10.0.0.128/25"); // 列 9（落在列 2 內）

    let csv = build_csv(&[
        ok,
        normalized,
        missing,
        invalid,
        duplicate_existing,
        overlap_existing,
        duplicate_file,
        overlap_file,
    ]);
    let (status, report) = post_csv(&pool, Some(true), csv.as_bytes()).await;
    assert_eq!(status, StatusCode::OK, "{report}");
    assert_eq!(report["summary"]["ok"], 2);
    assert_eq!(report["summary"]["errors"], 6);

    let row = row_at(&report, 2);
    assert_eq!(row["status"], "ok");
    assert_eq!(row["data"]["cidr"], "10.0.0.0/24");

    let row = row_at(&report, 3);
    assert_eq!(row["status"], "ok");
    assert_eq!(row["data"]["cidr"], "172.16.5.0/24");

    let row = row_at(&report, 4);
    assert_eq!(row["status"], "error");
    assert!(has_code(row, "cidr_required"), "{row}");
    assert!(row["data"]["cidr"].is_null());

    let row = row_at(&report, 5);
    assert_eq!(row["status"], "error");
    assert!(has_code(row, "invalid_cidr"), "{row}");

    let row = row_at(&report, 6);
    assert_eq!(row["status"], "error");
    assert!(has_code(row, "cidr_duplicate"), "{row}");
    assert!(message_of(row, "cidr_duplicate").contains("既有大網段"));

    let row = row_at(&report, 7);
    assert_eq!(row["status"], "error");
    assert!(has_code(row, "cidr_overlap"), "{row}");
    assert!(message_of(row, "cidr_overlap").contains("既有大網段"));

    let row = row_at(&report, 8);
    assert_eq!(row["status"], "error");
    assert!(has_code(row, "cidr_duplicate"), "{row}");
    assert!(message_of(row, "cidr_duplicate").contains("列 2"));

    let row = row_at(&report, 9);
    assert_eq!(row["status"], "error");
    assert!(has_code(row, "cidr_overlap"), "{row}");
    assert!(message_of(row, "cidr_overlap").contains("第 2 列"));
}

#[tokio::test]
async fn validates_gateway_rules() {
    let pool = test_pool().await;

    let mut inside = CsvRow::new("10.0.0.0/24");
    inside.gateway = "10.0.0.1".to_string(); // 列 2

    let mut malformed = CsvRow::new("10.0.1.0/24");
    malformed.gateway = "10.0.1".to_string(); // 列 3

    let mut outside = CsvRow::new("10.0.2.0/24");
    outside.gateway = "10.0.3.1".to_string(); // 列 4

    let mut v6_gateway = CsvRow::new("fd00::/64");
    v6_gateway.gateway = "fd00::1".to_string(); // 列 5

    let mut v4_gateway_on_v6 = CsvRow::new("fd01::/64");
    v4_gateway_on_v6.gateway = "10.0.0.1".to_string(); // 列 6

    let csv = build_csv(&[inside, malformed, outside, v6_gateway, v4_gateway_on_v6]);
    let (status, report) = post_csv(&pool, Some(true), csv.as_bytes()).await;
    assert_eq!(status, StatusCode::OK, "{report}");
    assert_eq!(report["summary"]["ok"], 2);
    assert_eq!(report["summary"]["errors"], 3);

    assert_eq!(row_at(&report, 2)["status"], "ok");
    assert_eq!(row_at(&report, 2)["data"]["gateway"], "10.0.0.1");

    let row = row_at(&report, 3);
    assert_eq!(row["status"], "error");
    assert!(has_code(row, "invalid_gateway"), "{row}");
    assert!(row["data"]["gateway"].is_null(), "無法解析者 data 為 null");

    let row = row_at(&report, 4);
    assert_eq!(row["status"], "error");
    assert!(has_code(row, "invalid_gateway"), "{row}");
    assert!(message_of(row, "invalid_gateway").contains("不在網段"));

    assert_eq!(row_at(&report, 5)["status"], "ok", "v6 gateway 允許");

    let row = row_at(&report, 6);
    assert_eq!(row["status"], "error");
    assert!(has_code(row, "invalid_gateway"), "{row}");
}

#[tokio::test]
async fn validates_kea_subnet_id_rules() {
    let pool = test_pool().await;
    create_subnet(
        &pool,
        json!({ "cidr": "10.99.0.0/24", "kea_subnet_id": 10 }),
    )
    .await;

    let mut ok = CsvRow::new("10.0.0.0/24");
    ok.kea_subnet_id = "1".to_string(); // 列 2

    let mut duplicate_file = CsvRow::new("10.0.1.0/24");
    duplicate_file.kea_subnet_id = "1".to_string(); // 列 3

    let mut duplicate_existing = CsvRow::new("10.0.2.0/24");
    duplicate_existing.kea_subnet_id = "10".to_string(); // 列 4

    let mut not_integer = CsvRow::new("10.0.3.0/24");
    not_integer.kea_subnet_id = "abc".to_string(); // 列 5

    let mut zero = CsvRow::new("10.0.4.0/24");
    zero.kea_subnet_id = "0".to_string(); // 列 6

    let mut negative = CsvRow::new("10.0.5.0/24");
    negative.kea_subnet_id = "-1".to_string(); // 列 7

    let mut v6_with_id = CsvRow::new("fd00::/64");
    v6_with_id.kea_subnet_id = "5".to_string(); // 列 8

    let csv = build_csv(&[
        ok,
        duplicate_file,
        duplicate_existing,
        not_integer,
        zero,
        negative,
        v6_with_id,
    ]);
    let (status, report) = post_csv(&pool, Some(true), csv.as_bytes()).await;
    assert_eq!(status, StatusCode::OK, "{report}");
    assert_eq!(report["summary"]["ok"], 1);
    assert_eq!(report["summary"]["errors"], 6);

    let row = row_at(&report, 2);
    assert_eq!(row["status"], "ok");
    assert_eq!(row["data"]["kea_subnet_id"], 1);

    let row = row_at(&report, 3);
    assert_eq!(row["status"], "error");
    assert!(has_code(row, "kea_subnet_id_duplicate"), "{row}");
    assert!(message_of(row, "kea_subnet_id_duplicate").contains("列 2"));

    let row = row_at(&report, 4);
    assert_eq!(row["status"], "error");
    assert!(has_code(row, "kea_subnet_id_duplicate"), "{row}");
    assert!(message_of(row, "kea_subnet_id_duplicate").contains("既有"));

    let row = row_at(&report, 5);
    assert_eq!(row["status"], "error");
    assert!(has_code(row, "invalid_kea_subnet_id"), "{row}");
    assert!(row["data"]["kea_subnet_id"].is_null());

    for row_number in [6, 7] {
        let row = row_at(&report, row_number);
        assert_eq!(row["status"], "error");
        assert!(has_code(row, "invalid_kea_subnet_id"), "{row}");
        assert!(message_of(row, "invalid_kea_subnet_id").contains("正整數"));
    }
    assert_eq!(
        row_at(&report, 6)["data"]["kea_subnet_id"],
        0,
        "可解析但非正整數仍保留原值於 data"
    );

    let row = row_at(&report, 8);
    assert_eq!(row["status"], "error");
    assert!(has_code(row, "kea_subnet_id_for_v6"), "{row}");
}

#[tokio::test]
async fn validates_pool_rules() {
    let pool = test_pool().await;

    let mut multi = CsvRow::new("10.0.0.0/24");
    multi.pools = "10.0.0.100-10.0.0.150|10.0.0.200-10.0.0.250".to_string(); // 列 2

    let mut malformed = CsvRow::new("10.0.1.0/24");
    malformed.pools = "10.0.1.100".to_string(); // 列 3

    let mut reversed = CsvRow::new("10.0.2.0/24");
    reversed.pools = "10.0.2.200-10.0.2.100".to_string(); // 列 4

    let mut outside = CsvRow::new("10.0.3.0/24");
    outside.pools = "10.0.4.100-10.0.4.200".to_string(); // 列 5

    let mut overlap = CsvRow::new("10.0.4.0/24");
    overlap.pools = "10.0.4.10-10.0.4.20|10.0.4.20-10.0.4.30".to_string(); // 列 6

    let mut empty_segment = CsvRow::new("10.0.5.0/24");
    empty_segment.pools = "10.0.5.100-10.0.5.150|".to_string(); // 列 7

    let mut v6_with_pool = CsvRow::new("fd00::/64");
    v6_with_pool.pools = "fd00::10-fd00::20".to_string(); // 列 8

    let csv = build_csv(&[
        multi,
        malformed,
        reversed,
        outside,
        overlap,
        empty_segment,
        v6_with_pool,
    ]);
    let (status, report) = post_csv(&pool, Some(true), csv.as_bytes()).await;
    assert_eq!(status, StatusCode::OK, "{report}");
    assert_eq!(report["summary"]["ok"], 1);
    assert_eq!(report["summary"]["errors"], 6);

    let row = row_at(&report, 2);
    assert_eq!(row["status"], "ok");
    assert_eq!(
        row["data"]["pools"],
        json!(["10.0.0.100-10.0.0.150", "10.0.0.200-10.0.0.250"])
    );

    for (row_number, fragment) in [
        (3, "格式錯誤"),
        (4, "不可大於"),
        (5, "不在網段"),
        (6, "重疊"),
        (7, "空段"),
    ] {
        let row = row_at(&report, row_number);
        assert_eq!(row["status"], "error", "第 {row_number} 列");
        assert!(has_code(row, "invalid_pools"), "{row}");
        assert!(
            message_of(row, "invalid_pools").contains(fragment),
            "第 {row_number} 列訊息應含 {fragment}：{row}"
        );
    }

    let row = row_at(&report, 8);
    assert_eq!(row["status"], "error");
    assert!(has_code(row, "pools_for_v6"), "{row}");
}

#[tokio::test]
async fn validates_exclusion_rules() {
    let pool = test_pool().await;

    let mut multi = CsvRow::new("10.0.0.0/24");
    multi.exclusions = "10.0.0.30-10.0.0.40#NAT 對外|10.0.0.50-10.0.0.50#說明#含井號".to_string(); // 列 2

    let mut malformed = CsvRow::new("10.0.1.0/24");
    malformed.exclusions = "10.0.1.30".to_string(); // 列 3

    let mut empty_segment = CsvRow::new("10.0.2.0/24");
    empty_segment.exclusions = "10.0.2.30-10.0.2.40|".to_string(); // 列 4

    let mut reversed = CsvRow::new("10.0.3.0/24");
    reversed.exclusions = "10.0.3.40-10.0.3.30".to_string(); // 列 5

    let mut outside = CsvRow::new("10.0.4.0/24");
    outside.exclusions = "10.0.5.10-10.0.5.20".to_string(); // 列 6

    let mut overlap_each_other = CsvRow::new("10.0.6.0/24");
    overlap_each_other.exclusions = "10.0.6.10-10.0.6.20|10.0.6.15-10.0.6.30".to_string(); // 列 7

    let mut overlap_pool = CsvRow::new("10.0.7.0/24");
    overlap_pool.pools = "10.0.7.10-10.0.7.20".to_string();
    overlap_pool.exclusions = "10.0.7.15-10.0.7.25".to_string(); // 列 8

    let csv = build_csv(&[
        multi,
        malformed,
        empty_segment,
        reversed,
        outside,
        overlap_each_other,
        overlap_pool,
    ]);
    let (status, report) = post_csv(&pool, Some(true), csv.as_bytes()).await;
    assert_eq!(status, StatusCode::OK, "{report}");
    assert_eq!(report["summary"]["ok"], 1);
    assert_eq!(report["summary"]["errors"], 6);

    let row = row_at(&report, 2);
    assert_eq!(row["status"], "ok");
    assert_eq!(
        row["data"]["exclusions"],
        json!([
            "10.0.0.30-10.0.0.40#NAT 對外",
            "10.0.0.50-10.0.0.50#說明#含井號"
        ]),
        "多段以 | 分隔；note 可含 #（以第一個 # 分隔）"
    );

    for (row_number, fragment) in [
        (3, "格式錯誤"),
        (4, "空段"),
        (5, "不可大於終點"),
        (6, "不在網段"),
        (7, "重疊"),
        (8, "不得與 DHCP 位址池重疊"),
    ] {
        let row = row_at(&report, row_number);
        assert_eq!(row["status"], "error", "第 {row_number} 列：{row}");
        assert!(has_code(row, "invalid_exclusions"), "{row}");
        assert_eq!(
            row["issues"][0]["field"], "exclusions",
            "問題欄位一律映射為 exclusions：{row}"
        );
        assert!(
            message_of(row, "invalid_exclusions").contains(fragment),
            "第 {row_number} 列訊息應含 {fragment}：{row}"
        );
    }
}

#[tokio::test]
async fn v6_exclusions_are_rejected() {
    let pool = test_pool().await;

    let mut v6 = CsvRow::new("fd00::/64");
    v6.exclusions = "fd00::10-fd00::20#NAT".to_string(); // 列 2

    // 空字串視為未填：v6 可正常匯入。
    let empty = CsvRow::new("fd01::/64"); // 列 3

    let csv = build_csv(&[v6, empty]);
    let (status, report) = post_csv(&pool, Some(true), csv.as_bytes()).await;
    assert_eq!(status, StatusCode::OK, "{report}");
    assert_eq!(report["summary"]["ok"], 1);
    assert_eq!(report["summary"]["errors"], 1);

    let row = row_at(&report, 2);
    assert_eq!(row["status"], "error");
    assert!(has_code(row, "exclusions_for_v6"), "{row}");
    assert_eq!(row["issues"][0]["field"], "exclusions");
    assert_eq!(
        row["data"]["exclusions"],
        json!(["fd00::10-fd00::20#NAT"]),
        "v6 錯誤列以原始段呈現"
    );

    assert_eq!(row_at(&report, 3)["status"], "ok", "v6 無排除範圍可建立");
}

#[tokio::test]
async fn legacy_six_column_csv_still_imports() {
    let pool = test_pool().await;

    // 舊檔：標題無「排除範圍」，欄位缺席視為未填。
    let text = "名稱,CIDR,Gateway,Kea subnet-id,位址池,備註\n\
                核心,10.0.0.0/24,10.0.0.1,1,10.0.0.100-10.0.0.150,主力\n\
                v6,fd00::/64,fd00::1,,,\n";
    let (status, report) = post_csv(&pool, Some(false), text.as_bytes()).await;
    assert_eq!(status, StatusCode::OK, "{report}");
    assert_eq!(report["summary"]["ok"], 2, "{report}");
    assert_eq!(report["summary"]["errors"], 0);
    assert_eq!(report["created"], json!({ "subnets": 2 }));
    assert_eq!(
        row_at(&report, 2)["data"]["exclusions"],
        json!([]),
        "欄位缺席視為未填"
    );

    // 正式寫入後僅既有欄位落地，無排除範圍。
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM subnets").await, 2);
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM subnet_pools").await, 1);
    assert_eq!(
        count(&pool, "SELECT COUNT(*) FROM subnet_exclusions").await,
        0
    );
}

#[tokio::test]
async fn export_then_import_round_trip_with_exclusions() {
    let source = test_pool().await;

    // 來源：多段排除範圍；用途說明含 `#`（往返以第一個 `#` 分隔）。
    create_subnet(
        &source,
        json!({
            "cidr": "10.0.0.0/24",
            "name": "辦公區",
            "note": "三樓",
            "gateway": "10.0.0.1",
            "kea_subnet_id": 1,
            "pools": [{ "start_ip": "10.0.0.100", "end_ip": "10.0.0.150" }],
            "exclusions": [
                { "start_ip": "10.0.0.30", "end_ip": "10.0.0.40", "note": "NAT #1" },
                { "start_ip": "10.0.0.50", "end_ip": "10.0.0.50" }
            ]
        }),
    )
    .await;
    create_subnet(&source, json!({ "cidr": "fd00::/64", "name": "v6 區" })).await;

    let (status, bytes) = send_bytes(&source, "/api/v1/subnets/export").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(&bytes[..3], &[0xEF, 0xBB, 0xBF], "須有 UTF-8 BOM");
    let csv = std::str::from_utf8(&bytes[3..]).expect("匯出內容為 UTF-8");

    // 匯入至全新資料庫：全數成功，排除範圍與用途說明正確還原。
    let target = test_pool().await;
    let (status, report) = post_csv(&target, Some(false), csv.as_bytes()).await;
    assert_eq!(status, StatusCode::OK, "{report}");
    assert_eq!(report["summary"]["ok"], 2, "{report}");
    assert_eq!(report["summary"]["errors"], 0);
    assert_eq!(report["created"], json!({ "subnets": 2 }));

    let row = row_at(&report, 2);
    assert_eq!(
        row["data"]["exclusions"],
        json!(["10.0.0.30-10.0.0.40#NAT #1", "10.0.0.50-10.0.0.50"]),
        "匯出→匯入往返：note 以第一個 # 分隔"
    );

    let (status, list) = send(&target, Method::GET, "/api/v1/subnets", None).await;
    assert_eq!(status, StatusCode::OK, "{list}");
    let id = list["items"]
        .as_array()
        .expect("items 為陣列")
        .iter()
        .find(|item| item["cidr"] == json!("10.0.0.0/24"))
        .and_then(|item| item["id"].as_i64())
        .expect("匯入後存在 10.0.0.0/24");

    let (status, detail) = send(&target, Method::GET, &format!("/api/v1/subnets/{id}"), None).await;
    assert_eq!(status, StatusCode::OK, "{detail}");
    assert_eq!(detail["name"], "辦公區");
    assert_eq!(detail["note"], "三樓");
    let exclusions = detail["exclusions"].as_array().expect("exclusions 為陣列");
    assert_eq!(exclusions.len(), 2, "{detail}");
    assert_eq!(exclusions[0]["start_ip"], "10.0.0.30");
    assert_eq!(exclusions[0]["end_ip"], "10.0.0.40");
    assert_eq!(exclusions[0]["note"], "NAT #1");
    assert_eq!(exclusions[1]["start_ip"], "10.0.0.50");
    assert_eq!(exclusions[1]["end_ip"], "10.0.0.50");
    assert!(exclusions[1]["note"].is_null(), "無用途說明者為 null");
}

#[tokio::test]
async fn dry_run_preview_does_not_write() {
    let pool = test_pool().await;

    let mut first = CsvRow::new("10.0.0.0/24");
    first.name = "核心".to_string();
    first.gateway = "10.0.0.1".to_string();
    first.kea_subnet_id = "1".to_string();
    first.pools = "10.0.0.100-10.0.0.150".to_string();
    first.exclusions = "10.0.0.30-10.0.0.40#NAT 對外".to_string();

    let second = CsvRow::new("fd00::/64");
    let csv = build_csv(&[first, second]);

    let (status, report) = post_csv(&pool, Some(true), csv.as_bytes()).await;
    assert_eq!(status, StatusCode::OK, "{report}");
    assert_eq!(report["dry_run"], true);
    assert_eq!(
        report["summary"],
        json!({ "total": 2, "ok": 2, "warnings": 0, "errors": 0 })
    );
    assert_eq!(report["committed"], false);
    assert!(report["created"].is_null());

    assert_eq!(count(&pool, "SELECT COUNT(*) FROM subnets").await, 0);
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM subnet_pools").await, 0);
    assert_eq!(
        count(&pool, "SELECT COUNT(*) FROM subnet_exclusions").await,
        0
    );
}

#[tokio::test]
async fn imports_subnets_in_one_transaction() {
    let pool = test_pool().await;

    let mut full = CsvRow::new("10.0.0.0/24");
    full.name = "核心".to_string();
    full.gateway = "10.0.0.1".to_string();
    full.kea_subnet_id = "1".to_string();
    full.pools = "10.0.0.100-10.0.0.150|10.0.0.200-10.0.0.250".to_string();
    full.exclusions = "10.0.0.30-10.0.0.40#NAT 對外|10.0.0.50-10.0.0.50".to_string();
    full.note = "主力".to_string();

    let mut v6 = CsvRow::new("fd00::/64");
    v6.name = "v6 區".to_string();
    v6.gateway = "fd00::1".to_string();

    let minimal = CsvRow::new("172.16.0.0/16");

    let csv = build_csv(&[full, v6, minimal]);

    // 先預覽：不寫任何資料。
    let (status, preview) = post_csv(&pool, Some(true), csv.as_bytes()).await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    assert_eq!(preview["summary"]["ok"], 3);
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM subnets").await, 0);

    // 正式匯入：單一交易建立全部網段與 pools。
    let (status, report) = post_csv(&pool, Some(false), csv.as_bytes()).await;
    assert_eq!(status, StatusCode::OK, "{report}");
    assert_eq!(report["dry_run"], false);
    assert_eq!(report["committed"], true, "{report}");
    assert_eq!(report["created"], json!({ "subnets": 3 }));

    assert_eq!(count(&pool, "SELECT COUNT(*) FROM subnets").await, 3);
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM subnet_pools").await, 2);
    assert_eq!(
        count(&pool, "SELECT COUNT(*) FROM subnet_exclusions").await,
        2
    );

    // 以既有 GET 端點驗證落地內容。
    let (status, list) = send(&pool, Method::GET, "/api/v1/subnets", None).await;
    assert_eq!(status, StatusCode::OK, "{list}");
    let id_of = |cidr: &str| {
        list["items"]
            .as_array()
            .expect("items 為陣列")
            .iter()
            .find(|item| item["cidr"] == json!(cidr))
            .and_then(|item| item["id"].as_i64())
            .unwrap_or_else(|| panic!("找不到網段 {cidr}"))
    };

    let (status, detail) = send(
        &pool,
        Method::GET,
        &format!("/api/v1/subnets/{}", id_of("10.0.0.0/24")),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{detail}");
    assert_eq!(detail["name"], "核心");
    assert_eq!(detail["gateway"], "10.0.0.1");
    assert_eq!(detail["kea_subnet_id"], 1);
    assert_eq!(detail["note"], "主力");
    let pools: Vec<String> = detail["pools"]
        .as_array()
        .expect("pools 為陣列")
        .iter()
        .map(|pool| {
            format!(
                "{}-{}",
                pool["start_ip"].as_str().unwrap(),
                pool["end_ip"].as_str().unwrap()
            )
        })
        .collect();
    assert_eq!(
        pools,
        ["10.0.0.100-10.0.0.150", "10.0.0.200-10.0.0.250"],
        "pool 順序與輸入一致"
    );

    let exclusions: Vec<String> = detail["exclusions"]
        .as_array()
        .expect("exclusions 為陣列")
        .iter()
        .map(|exclusion| match exclusion["note"].as_str() {
            Some(note) => format!(
                "{}-{}#{}",
                exclusion["start_ip"].as_str().unwrap(),
                exclusion["end_ip"].as_str().unwrap(),
                note
            ),
            None => format!(
                "{}-{}",
                exclusion["start_ip"].as_str().unwrap(),
                exclusion["end_ip"].as_str().unwrap()
            ),
        })
        .collect();
    assert_eq!(
        exclusions,
        ["10.0.0.30-10.0.0.40#NAT 對外", "10.0.0.50-10.0.0.50"],
        "排除範圍與用途說明落地且順序與輸入一致"
    );

    let (status, detail) = send(
        &pool,
        Method::GET,
        &format!("/api/v1/subnets/{}", id_of("fd00::/64")),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{detail}");
    assert_eq!(detail["gateway"], "fd00::1");
    assert_eq!(detail["kea_subnet_id"], Value::Null, "v6 無 Kea subnet-id");
    assert_eq!(detail["pools"], json!([]), "v6 無 pool");

    let (_, detail) = send(
        &pool,
        Method::GET,
        &format!("/api/v1/subnets/{}", id_of("172.16.0.0/16")),
        None,
    )
    .await;
    assert_eq!(detail["name"], Value::Null, "選填欄位缺值為 null");
    assert_eq!(detail["kea_subnet_id"], Value::Null);
    assert_eq!(detail["pools"], json!([]));
}

#[tokio::test]
async fn failed_import_writes_nothing() {
    let pool = test_pool().await;

    let valid = CsvRow::new("10.0.0.0/24"); // 列 2
    let mut invalid = CsvRow::new("10.0.0.128/25"); // 列 3：與列 2 重疊
    invalid.name = "錯誤網段".to_string();

    let (status, report) =
        post_csv(&pool, Some(false), build_csv(&[valid, invalid]).as_bytes()).await;
    assert_eq!(status, StatusCode::OK, "驗證報告仍為 200：{report}");
    assert_eq!(report["committed"], false);
    assert!(report["created"].is_null());
    assert_eq!(report["summary"]["errors"], 1);
    assert!(has_code(row_at(&report, 3), "cidr_overlap"));

    // 全有全無：無任何殘留。
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM subnets").await, 0);
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM subnet_pools").await, 0);
    assert_eq!(
        count(&pool, "SELECT COUNT(*) FROM subnet_exclusions").await,
        0
    );
}

#[tokio::test]
async fn external_change_after_preview_blocks_commit() {
    let pool = test_pool().await;

    let csv = build_csv(&[CsvRow::new("10.0.0.0/24")]);

    // 預覽當下無錯誤。
    let (status, preview) = post_csv(&pool, Some(true), csv.as_bytes()).await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    assert_eq!(preview["summary"]["errors"], 0);

    // 背後他人先建立同 CIDR 網段。
    create_subnet(&pool, json!({ "cidr": "10.0.0.0/24", "name": "他人建立" })).await;

    // 正式匯入以當下資料重驗：committed=false（驗證報告）或 400（唯一性衝突）皆可，但不得寫入。
    let (status, body) = post_csv(&pool, Some(false), csv.as_bytes()).await;
    if status == StatusCode::OK {
        assert_eq!(body["committed"], false, "{body}");
        assert!(
            body["summary"]["errors"].as_u64().unwrap_or(0) >= 1,
            "{body}"
        );
        assert!(body["created"].is_null());
    } else {
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    }

    assert_eq!(
        count(&pool, "SELECT COUNT(*) FROM subnets").await,
        1,
        "僅既有網段"
    );
    let name: Option<String> = sqlx::query_scalar("SELECT name FROM subnets")
        .fetch_one(&pool)
        .await
        .expect("讀取既有網段");
    assert_eq!(name.as_deref(), Some("他人建立"), "原有網段未被異動");
}
