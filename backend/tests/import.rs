//! 資產 CSV 匯入整合測試：編碼、標題、14 欄規則、語意警示、上限、
//! dry_run 預覽與單一交易寫入（見票 02）。

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

const BOUNDARY: &str = "asset-nest-import-boundary";

/// 以 multipart（檔案欄位 `file`）呼叫匯入端點；`dry_run` 為 `None` 時不帶查詢參數。
async fn post_csv(pool: &SqlitePool, dry_run: Option<bool>, bytes: &[u8]) -> (StatusCode, Value) {
    let uri = match dry_run {
        Some(value) => format!("/api/v1/assets/import?dry_run={value}"),
        None => "/api/v1/assets/import".to_string(),
    };

    let mut body = Vec::new();
    body.extend_from_slice(
        format!(
            "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"file\"; \
             filename=\"assets.csv\"\r\nContent-Type: text/csv\r\n\r\n"
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

/// 匯入 CSV 標題列（14 欄，順序同 ADR-0008）。
const HEADERS: &str =
    "財產編號,描述,位置,設備序號,廠牌,型號,購置日期,年限,備註,標籤,MAC,IPv4,IPv6,hostname";

/// 14 欄資料列；預設僅填描述與位置，其餘留空。
#[derive(Default)]
struct CsvRow {
    property_no: String,
    description: String,
    location: String,
    device_serial: String,
    brand: String,
    model: String,
    purchase_date: String,
    lifespan: String,
    note: String,
    tags: String,
    mac: String,
    ipv4: String,
    ipv6: String,
    hostname: String,
}

impl CsvRow {
    fn new(description: &str, location: &str) -> Self {
        Self {
            description: description.to_string(),
            location: location.to_string(),
            ..Self::default()
        }
    }

    fn fields(&self) -> [&str; 14] {
        [
            &self.property_no,
            &self.description,
            &self.location,
            &self.device_serial,
            &self.brand,
            &self.model,
            &self.purchase_date,
            &self.lifespan,
            &self.note,
            &self.tags,
            &self.mac,
            &self.ipv4,
            &self.ipv6,
            &self.hostname,
        ]
    }
}

/// 以標準 14 欄標題組出 CSV 文字。
fn build_csv(rows: &[CsvRow]) -> String {
    let mut text = String::from(HEADERS);
    for row in rows {
        text.push('\n');
        text.push_str(&row.fields().join(","));
    }
    text.push('\n');
    text
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

/// 新增網段並斷言成功，回傳 id；可帶一段 v4 pool。
async fn create_subnet(pool: &SqlitePool, cidr: &str, pool_range: Option<(&str, &str)>) -> i64 {
    let mut body = json!({ "cidr": cidr });
    if let Some((start, end)) = pool_range {
        body["pools"] = json!([{ "start_ip": start, "end_ip": end }]);
    }
    let (status, json) = send(pool, Method::POST, "/api/v1/subnets", Some(body)).await;
    assert_eq!(status, StatusCode::CREATED, "新增網段應成功：{json}");
    json["id"].as_i64().expect("回應含 id")
}

/// 指派位址並斷言成功。
async fn assign_ip(
    pool: &SqlitePool,
    subnet_id: i64,
    address: &str,
    interface_id: i64,
    purpose: &str,
) {
    let (status, json) = send(
        pool,
        Method::PUT,
        &format!("/api/v1/subnets/{subnet_id}/ips/{address}/assignment"),
        Some(json!({ "interface_id": interface_id, "purpose": purpose })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "指派應成功：{json}");
}

/// 依描述／財產編號搜尋並回傳第一筆資產 id。
async fn find_asset(pool: &SqlitePool, query: &str) -> i64 {
    let (status, page) = send(
        pool,
        Method::GET,
        &format!("/api/v1/assets?q={query}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    page["items"][0]["id"].as_i64().expect("找到資產")
}

/// 讀取資產詳情（含介面與指派）並斷言成功。
async fn asset_detail(pool: &SqlitePool, asset_id: i64) -> Value {
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

    // 靜態路徑 `/assets/import` 須命中匯入端點，而非 `/assets/{id}`。
    let csv = build_csv(&[CsvRow::new("測試機", "機房A")]);
    let (status, report) = post_csv(&pool, None, csv.as_bytes()).await;
    assert_eq!(status, StatusCode::OK, "匯入端點應回 200：{report}");
    assert_eq!(report["dry_run"], true, "dry_run 預設 true");
    assert_eq!(report["summary"]["total"], 1);
    assert_eq!(report["summary"]["ok"], 1);
    assert_eq!(report["committed"], false);
    assert!(report["created"].is_null());

    // 非 multipart 請求也進到匯入端點（動態路由無 POST，會是 405）。
    let (status, body) = send(
        &pool,
        Method::POST,
        "/api/v1/assets/import",
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
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM assets").await, 0);
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM interfaces").await, 0);

    // 缺少 file 欄位 → 400。
    let body = format!(
        "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"other\"\r\n\r\nx\r\n--{BOUNDARY}--\r\n"
    )
    .into_bytes();
    let (status, body) = send_raw(
        &pool,
        Method::POST,
        "/api/v1/assets/import",
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
        "/api/v1/assets/import?dry_run=maybe",
        &format!("multipart/form-data; boundary={BOUNDARY}"),
        Vec::new(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[tokio::test]
async fn headers_are_trimmed_case_insensitive_and_order_free() {
    let pool = test_pool().await;
    create_subnet(&pool, "10.0.0.0/24", None).await;

    // 未知標題忽略、英文不分大小寫、順序不拘、單元格 trim。
    let text = " 位置 ,數量,描述, mac ,IPv4,HOSTNAME\n 機房A ,3, 機台 ,AA.BB.CC.DD.EE.FF,10.0.0.5,srv-x \n";
    let (status, report) = post_csv(&pool, Some(true), text.as_bytes()).await;
    assert_eq!(status, StatusCode::OK, "{report}");
    assert_eq!(report["ignored_headers"], json!(["數量"]));
    assert_eq!(report["summary"]["errors"], 0);

    let row = row_at(&report, 2);
    assert_eq!(row["status"], "ok");
    assert_eq!(row["data"]["description"], "機台");
    assert_eq!(row["data"]["location"], "機房A");
    assert_eq!(row["data"]["mac"], "aa:bb:cc:dd:ee:ff");
    assert_eq!(row["data"]["ipv4"], "10.0.0.5");
    assert_eq!(row["data"]["hostname"], "srv-x");
}

#[tokio::test]
async fn missing_or_duplicate_headers_return_400() {
    let pool = test_pool().await;

    let (status, body) = post_csv(&pool, Some(true), "位置\n機台\n".as_bytes()).await;
    // 缺「描述」標題。
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], "validation_error");
    let message = body["message"].as_str().expect("訊息為字串");
    assert!(message.contains("缺少必要標題"), "{message}");
    assert!(message.contains("描述"), "{message}");
    assert_eq!(body["details"]["field"], "file");

    let (status, body) = post_csv(
        &pool,
        Some(true),
        "描述,描述,位置\n機台,機台,機房\n".as_bytes(),
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
        "描述,位置,數量,數量\n機台,機房,1,2\n".as_bytes(),
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
    let csv = build_csv(&[CsvRow::new("測試機", "機房A")]);
    let mut with_bom = vec![0xEF, 0xBB, 0xBF];
    with_bom.extend_from_slice(csv.as_bytes());
    let (status, report) = post_csv(&pool, Some(true), &with_bom).await;
    assert_eq!(status, StatusCode::OK, "{report}");
    assert_eq!(report["encoding"], "utf-8");
    assert_eq!(row_at(&report, 2)["data"]["description"], "測試機");

    // Big5：以 bytes 造檔並偵測。
    let source = "財產編號,描述,位置\nPC-B5,測試機,機房A\n";
    let (big5, _, _) = encoding_rs::BIG5.encode(source);
    let (status, report) = post_csv(&pool, Some(true), &big5).await;
    assert_eq!(status, StatusCode::OK, "{report}");
    assert_eq!(report["encoding"], "big5");
    assert_eq!(report["summary"]["ok"], 1);
    assert_eq!(row_at(&report, 2)["data"]["property_no"], "PC-B5");
    assert_eq!(row_at(&report, 2)["data"]["description"], "測試機");

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

    // 超過 5,000 列。
    let mut text = String::from("描述,位置\n");
    for index in 0..5_001 {
        text.push_str(&format!("機台{index},機房\n"));
    }
    let (status, body) = post_csv(&pool, Some(true), text.as_bytes()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(
        body["message"].as_str().expect("訊息").contains("5,000"),
        "{body}"
    );

    // 剛好 5,000 列可進入預覽（不寫入）。
    let mut text = String::from("描述,位置\n");
    for index in 0..5_000 {
        text.push_str(&format!("機台{index},機房\n"));
    }
    let (status, report) = post_csv(&pool, Some(true), text.as_bytes()).await;
    assert_eq!(status, StatusCode::OK, "{report}");
    assert_eq!(report["summary"]["total"], 5_000);

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
async fn empty_lines_and_rfc4180_quotes_are_handled() {
    let pool = test_pool().await;

    // 空行與全空白列忽略；引號內逗號與換行屬同一列（RFC 4180）。
    let text = "描述,位置,備註\n\n\"伺服器, 第一台\n第二行\",機房A,備註\n   \n機台乙,機房B,\n";
    let (status, report) = post_csv(&pool, Some(true), text.as_bytes()).await;
    assert_eq!(status, StatusCode::OK, "{report}");
    assert_eq!(report["summary"]["total"], 2);
    assert_eq!(
        row_at(&report, 2)["data"]["description"],
        "伺服器, 第一台\n第二行"
    );
    assert_eq!(row_at(&report, 3)["data"]["description"], "機台乙");
}

#[tokio::test]
async fn column_count_mismatch_is_a_row_error() {
    let pool = test_pool().await;

    let mut csv = String::from(HEADERS);
    csv.push_str("\n機台甲,機房A\n"); // 欄位過少
    csv.push_str(&format!("{}\n", ["機台乙", "機房B"].repeat(8).join(","))); // 欄位過多

    let (status, report) = post_csv(&pool, Some(false), csv.as_bytes()).await;
    assert_eq!(status, StatusCode::OK, "{report}");
    assert_eq!(report["summary"]["errors"], 2);
    assert_eq!(report["committed"], false);
    assert!(report["created"].is_null());

    let row = row_at(&report, 2);
    assert_eq!(row["status"], "error");
    assert!(has_code(row, "column_count_mismatch"));
    assert!(message_of(row, "column_count_mismatch").contains("14"));

    // 整批不匯入，無殘留。
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM assets").await, 0);
}

#[tokio::test]
async fn validates_asset_fields_and_normalizes_values() {
    let pool = test_pool().await;

    let mut full = CsvRow::new("伺服器甲", "機房A");
    full.property_no = "PC-100".to_string();
    full.device_serial = "SN-100".to_string();
    full.brand = "Dell".to_string();
    full.model = "R740".to_string();
    full.purchase_date = "2024/1/15".to_string();
    full.lifespan = "5".to_string();
    full.note = "主力".to_string();
    full.tags = " 核心 | 伺服器 | 核心 ".to_string();

    let mut dotted = CsvRow::new("電腦乙", "辦公室B");
    dotted.purchase_date = "2024.1.5".to_string();
    dotted.lifespan = "0".to_string();
    dotted.tags = "Core|core".to_string();

    let mut missing = CsvRow::new("", "機房A");
    missing.lifespan = "abc".to_string();

    let mut negative = CsvRow::new("設備丙", "機房C");
    negative.lifespan = "-1".to_string();

    let mut bad_date = CsvRow::new("設備丁", "機房D");
    bad_date.purchase_date = "2024/13/01".to_string();

    let missing_location = CsvRow::new("設備戊", "");

    let csv = build_csv(&[full, dotted, missing, negative, bad_date, missing_location]);
    let (status, report) = post_csv(&pool, Some(true), csv.as_bytes()).await;
    assert_eq!(status, StatusCode::OK, "{report}");

    let row = row_at(&report, 2);
    assert_eq!(row["status"], "ok", "{row}");
    assert_eq!(row["data"]["purchase_date"], "2024-01-15");
    assert_eq!(row["data"]["lifespan_years"], 5);
    assert_eq!(row["data"]["tags"], json!(["核心", "伺服器"]));
    assert_eq!(row["data"]["property_no"], "PC-100");

    let row = row_at(&report, 3);
    assert_eq!(row["status"], "ok", "{row}");
    assert_eq!(row["data"]["purchase_date"], "2024-01-05");
    assert_eq!(row["data"]["tags"], json!(["Core"]), "標籤不分大小寫去重");

    let row = row_at(&report, 4);
    assert_eq!(row["status"], "error");
    assert!(has_code(row, "description_required"));
    assert!(has_code(row, "invalid_lifespan"), "{row}");
    assert_eq!(row["data"]["lifespan_years"], Value::Null);

    let row = row_at(&report, 5);
    assert_eq!(row["status"], "error");
    assert!(has_code(row, "invalid_lifespan"));
    assert!(message_of(row, "invalid_lifespan").contains("非負整數"));

    let row = row_at(&report, 6);
    assert_eq!(row["status"], "error");
    assert!(has_code(row, "invalid_purchase_date"));
    assert_eq!(row["data"]["purchase_date"], Value::Null);

    let row = row_at(&report, 7);
    assert_eq!(row["status"], "error");
    assert!(has_code(row, "location_required"));
}

#[tokio::test]
async fn validates_mac_format_and_normalizes() {
    let pool = test_pool().await;

    let mut valid = CsvRow::new("設備甲", "機房A");
    valid.mac = "AA-BB-CC-DD-EE-01".to_string();

    let mut invalid = CsvRow::new("設備乙", "機房A");
    invalid.mac = "gg:hh:ii:jj:kk:ll".to_string();

    let csv = build_csv(&[valid, invalid]);
    let (status, report) = post_csv(&pool, Some(true), csv.as_bytes()).await;
    assert_eq!(status, StatusCode::OK, "{report}");

    assert_eq!(row_at(&report, 2)["data"]["mac"], "aa:bb:cc:dd:ee:01");
    let row = row_at(&report, 3);
    assert_eq!(row["status"], "error");
    assert!(has_code(row, "invalid_mac"));
    assert_eq!(row["issues"][0]["field"], "mac");
}

#[tokio::test]
async fn validates_ipv4_rules() {
    let pool = test_pool().await;
    create_subnet(&pool, "10.0.0.0/24", Some(("10.0.0.100", "10.0.0.150"))).await;

    let cases: [(&str, &str); 5] = [
        ("10.0.0.0", "ipv4_not_host"),
        ("10.0.0.255", "ipv4_not_host"),
        ("10.0.0.120", "ipv4_in_pool"),
        ("10.9.9.9", "ipv4_out_of_subnet"),
        ("999.1.1.1", "invalid_ipv4"),
    ];

    let mut rows = Vec::new();
    let mut normal = CsvRow::new("正常", "機房A");
    normal.ipv4 = "10.0.0.5".to_string();
    rows.push(normal);
    for (address, _) in cases {
        let mut row = CsvRow::new("測試", "機房A");
        row.ipv4 = address.to_string();
        rows.push(row);
    }

    let (status, report) = post_csv(&pool, Some(true), build_csv(&rows).as_bytes()).await;
    assert_eq!(status, StatusCode::OK, "{report}");

    assert_eq!(row_at(&report, 2)["status"], "ok");
    assert_eq!(row_at(&report, 2)["data"]["ipv4"], "10.0.0.5");
    for (index, (address, code)) in cases.iter().enumerate() {
        let row = row_at(&report, index + 3);
        assert_eq!(row["status"], "error", "第 {index} 個案例 {address}");
        assert!(has_code(row, code), "案例 {address} 應為 {code}：{row}");
    }
}

#[tokio::test]
async fn ipv4_assigned_error_includes_current_target() {
    let pool = test_pool().await;
    let subnet_id = create_subnet(&pool, "10.0.0.0/24", None).await;

    let asset_id = create_asset(&pool, "資料庫主機", "機房C").await;
    let interface_id = create_interface(
        &pool,
        asset_id,
        json!({ "name": "eth0", "mac": "aa:bb:cc:dd:ee:aa" }),
    )
    .await;
    assign_ip(&pool, subnet_id, "10.0.0.40", interface_id, "static").await;

    let mut row = CsvRow::new("新主機", "機房D");
    row.ipv4 = "10.0.0.40".to_string();

    let (status, report) = post_csv(&pool, Some(true), build_csv(&[row]).as_bytes()).await;
    assert_eq!(status, StatusCode::OK, "{report}");

    let row = row_at(&report, 2);
    assert_eq!(row["status"], "error");
    assert!(has_code(row, "ipv4_assigned"), "{row}");
    let message = message_of(row, "ipv4_assigned");
    for fragment in ["資料庫主機", "機房C", "eth0", "aa:bb:cc:dd:ee:aa"] {
        assert!(message.contains(fragment), "訊息應含 {fragment}：{message}");
    }
}

#[tokio::test]
async fn duplicate_ipv4_in_file_is_an_error() {
    let pool = test_pool().await;
    create_subnet(&pool, "10.0.0.0/24", None).await;

    let mut first = CsvRow::new("設備一", "機房A");
    first.ipv4 = "10.0.0.7".to_string();
    let mut second = CsvRow::new("設備二", "機房A");
    second.ipv4 = "10.0.0.7".to_string();

    let (status, report) =
        post_csv(&pool, Some(true), build_csv(&[first, second]).as_bytes()).await;
    assert_eq!(status, StatusCode::OK, "{report}");

    assert_eq!(row_at(&report, 2)["status"], "ok");
    let row = row_at(&report, 3);
    assert_eq!(row["status"], "error");
    assert!(has_code(row, "ipv4_duplicate"), "{row}");
    assert!(message_of(row, "ipv4_duplicate").contains("列 2"));
}

#[tokio::test]
async fn validates_ipv6_rules() {
    let pool = test_pool().await;
    let subnet_id = create_subnet(&pool, "fd00::/64", None).await;

    let asset_id = create_asset(&pool, "既有設備", "機房A").await;
    let interface_id = create_interface(&pool, asset_id, json!({ "name": "eth0" })).await;
    assign_ip(&pool, subnet_id, "fd00::20", interface_id, "static").await;

    let mut cases = vec![
        ("fd00::10", Some("ok")),
        ("fd00::", Some("ok")), // 登錄制含 network 位址
        ("fd99::1", Some("ipv6_out_of_subnet")),
        ("10.0.0.5", Some("invalid_ipv6")),
        ("fd00::20", Some("ipv6_registered")),
    ];
    let mut rows: Vec<CsvRow> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for (address, _) in &cases {
        if seen.insert(address.to_string()) {
            let mut row = CsvRow::new("測試", "機房A");
            row.ipv6 = address.to_string();
            rows.push(row);
        }
    }
    // 檔內重複：fd00::10 再一列。
    let mut duplicate = CsvRow::new("重複", "機房A");
    duplicate.ipv6 = "fd00::10".to_string();
    rows.push(duplicate);
    cases.push(("fd00::10", None));

    let (status, report) = post_csv(&pool, Some(true), build_csv(&rows).as_bytes()).await;
    assert_eq!(status, StatusCode::OK, "{report}");

    for (index, (address, expected)) in cases.iter().enumerate() {
        let row = row_at(&report, index + 2);
        match expected {
            Some(code) if *code != "ok" => {
                assert_eq!(row["status"], "error", "{address}");
                assert!(has_code(row, code), "{address} 應為 {code}：{row}");
            }
            Some(_) => assert_eq!(row["status"], "ok", "{address}：{row}"),
            None => {
                assert!(has_code(row, "ipv6_duplicate"), "{address}：{row}");
            }
        }
    }
    assert!(message_of(row_at(&report, 6), "ipv6_registered").contains("既有設備"));
}

#[tokio::test]
async fn hostname_requires_mac_and_ipv4() {
    let pool = test_pool().await;
    create_subnet(&pool, "10.0.0.0/24", None).await;

    let mut reservation = CsvRow::new("保留列", "機房A");
    reservation.mac = "aa:bb:cc:dd:ee:10".to_string();
    reservation.ipv4 = "10.0.0.10".to_string();
    reservation.hostname = "srv-10".to_string();

    let mut ipv4_only = CsvRow::new("無 MAC", "機房A");
    ipv4_only.ipv4 = "10.0.0.11".to_string();
    ipv4_only.hostname = "srv-11".to_string();

    let mut mac_only = CsvRow::new("無 IP", "機房A");
    mac_only.mac = "aa:bb:cc:dd:ee:11".to_string();
    mac_only.hostname = "srv-12".to_string();

    let mut alone = CsvRow::new("只有 hostname", "機房A");
    alone.hostname = "srv-13".to_string();

    let csv = build_csv(&[reservation, ipv4_only, mac_only, alone]);
    let (status, report) = post_csv(&pool, Some(true), csv.as_bytes()).await;
    assert_eq!(status, StatusCode::OK, "{report}");

    assert_eq!(row_at(&report, 2)["status"], "ok");
    for row_number in [3, 4, 5] {
        let row = row_at(&report, row_number);
        assert_eq!(row["status"], "error", "第 {row_number} 列");
        assert!(has_code(row, "hostname_not_allowed"), "{row}");
    }
}

#[tokio::test]
async fn duplicate_values_warn_without_blocking() {
    let pool = test_pool().await;

    let existing_id = create_asset(&pool, "既有設備", "機房A").await;
    send(
        &pool,
        Method::PATCH,
        &format!("/api/v1/assets/{existing_id}"),
        Some(json!({ "property_no": "PC-9", "device_serial": "SN-9" })),
    )
    .await;
    create_interface(
        &pool,
        existing_id,
        json!({ "name": "eth0", "mac": "aa:bb:cc:dd:ee:99" }),
    )
    .await;

    let mut hit_existing = CsvRow::new("新設備", "機房A");
    hit_existing.property_no = "pc-9".to_string();
    hit_existing.device_serial = "sn-9".to_string();
    hit_existing.mac = "AA:BB:CC:DD:EE:99".to_string();

    let mut first = CsvRow::new("檔案一", "機房A");
    first.property_no = "PC-1".to_string();
    first.device_serial = "SN-1".to_string();
    first.mac = "aa:bb:cc:dd:ee:01".to_string();

    let mut second = CsvRow::new("檔案二", "機房A");
    second.property_no = "PC-1".to_string();
    second.device_serial = "SN-1".to_string();
    second.mac = "AA-BB-CC-DD-EE-01".to_string();

    let csv = build_csv(&[hit_existing, first, second]);
    let (status, report) = post_csv(&pool, Some(true), csv.as_bytes()).await;
    assert_eq!(status, StatusCode::OK, "{report}");

    assert_eq!(report["summary"]["warnings"], 2, "{report}");
    let row = row_at(&report, 2);
    assert_eq!(row["status"], "warning");
    for code in [
        "duplicate_property_no",
        "duplicate_device_serial",
        "duplicate_mac",
    ] {
        assert!(has_code(row, code), "既有值應警示 {code}：{row}");
    }

    let row = row_at(&report, 3);
    assert_eq!(row["status"], "ok", "首次出現不警示：{row}");
    let row = row_at(&report, 4);
    assert_eq!(row["status"], "warning");
    for code in [
        "duplicate_property_no",
        "duplicate_device_serial",
        "duplicate_mac",
    ] {
        assert!(has_code(row, code), "檔內重複應警示 {code}：{row}");
    }

    // 警示不擋匯入：dry_run=false 仍寫入。
    let (status, report) = post_csv(&pool, Some(false), csv.as_bytes()).await;
    assert_eq!(status, StatusCode::OK, "{report}");
    assert_eq!(report["committed"], true, "{report}");
    assert_eq!(report["created"]["assets"], 3);
}

#[tokio::test]
async fn imports_assets_interfaces_and_assignments_in_one_transaction() {
    let pool = test_pool().await;
    create_subnet(&pool, "10.0.0.0/24", Some(("10.0.0.100", "10.0.0.150"))).await;
    create_subnet(&pool, "fd00::/64", None).await;

    let mut full = CsvRow::new("伺服器甲", "機房A");
    full.property_no = "PC-IMP-1".to_string();
    full.device_serial = "SN-IMP-1".to_string();
    full.brand = "Dell".to_string();
    full.model = "R740".to_string();
    full.purchase_date = "2024/1/15".to_string();
    full.lifespan = "5".to_string();
    full.note = "主力".to_string();
    full.tags = "核心|伺服器".to_string();
    full.mac = "AA-BB-CC-DD-EE-0A".to_string();
    full.ipv4 = "10.0.0.10".to_string();
    full.hostname = "srv-a".to_string();

    let mut reserved = CsvRow::new("電腦乙", "辦公室B");
    reserved.property_no = "PC-IMP-2".to_string();
    reserved.ipv4 = "10.0.0.20".to_string();

    let mut v6 = CsvRow::new("設備丙", "機房C");
    v6.property_no = "PC-IMP-3".to_string();
    v6.ipv6 = "fd00::10".to_string();

    let mut mac_only = CsvRow::new("設備丁", "機房D");
    mac_only.property_no = "PC-IMP-4".to_string();
    mac_only.mac = "aa:bb:cc:dd:ee:0d".to_string();

    let mut asset_only = CsvRow::new("設備戊", "倉庫");
    asset_only.property_no = "PC-IMP-5".to_string();

    let csv = build_csv(&[full, reserved, v6, mac_only, asset_only]);

    // 先預覽：不寫任何資料。
    let (status, preview) = post_csv(&pool, Some(true), csv.as_bytes()).await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    assert_eq!(
        preview["summary"],
        json!({ "total": 5, "ok": 5, "warnings": 0, "errors": 0 })
    );
    assert_eq!(preview["committed"], false);
    assert!(preview["created"].is_null());
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM assets").await, 0);
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM interfaces").await, 0);
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM ip_assignments").await, 0);

    // 正式匯入：單一交易建立資產＋eth0＋指派。
    let (status, report) = post_csv(&pool, Some(false), csv.as_bytes()).await;
    assert_eq!(status, StatusCode::OK, "{report}");
    assert_eq!(report["dry_run"], false);
    assert_eq!(report["committed"], true, "{report}");
    assert_eq!(
        report["created"],
        json!({ "assets": 5, "interfaces": 4, "assignments": 3 })
    );

    // 以既有 GET 端點驗證落地內容。
    let id = find_asset(&pool, "PC-IMP-1").await;
    let detail = asset_detail(&pool, id).await;
    assert_eq!(detail["purchase_date"], "2024-01-15");
    assert_eq!(detail["tags"], json!(["核心", "伺服器"]));
    assert_eq!(detail["interfaces"][0]["name"], "eth0");
    assert_eq!(detail["interfaces"][0]["mac"], "aa:bb:cc:dd:ee:0a");
    assert_eq!(detail["assignments"][0]["address"], "10.0.0.10");
    assert_eq!(detail["assignments"][0]["purpose"], "reservation");
    assert_eq!(detail["assignments"][0]["hostname"], "srv-a");

    let id = find_asset(&pool, "PC-IMP-2").await;
    let detail = asset_detail(&pool, id).await;
    assert_eq!(detail["interfaces"][0]["name"], "eth0");
    assert_eq!(
        detail["interfaces"][0]["mac"],
        Value::Null,
        "無 MAC 的手動介面"
    );
    assert_eq!(detail["assignments"][0]["purpose"], "static");
    assert_eq!(detail["assignments"][0]["hostname"], Value::Null);

    let id = find_asset(&pool, "PC-IMP-3").await;
    let detail = asset_detail(&pool, id).await;
    assert_eq!(detail["assignments"][0]["address"], "fd00::10");
    assert_eq!(detail["assignments"][0]["purpose"], "static");

    let id = find_asset(&pool, "PC-IMP-4").await;
    let detail = asset_detail(&pool, id).await;
    assert_eq!(detail["interfaces"][0]["mac"], "aa:bb:cc:dd:ee:0d");
    assert_eq!(detail["assignments"], json!([]), "僅 MAC 不建指派");

    let id = find_asset(&pool, "PC-IMP-5").await;
    let detail = asset_detail(&pool, id).await;
    assert_eq!(detail["interfaces"], json!([]), "全空只建資產");
}

#[tokio::test]
async fn failed_import_writes_nothing() {
    let pool = test_pool().await;
    create_subnet(&pool, "10.0.0.0/24", None).await;

    let mut valid = CsvRow::new("正常設備", "機房A");
    valid.property_no = "PC-OK".to_string();
    valid.ipv4 = "10.0.0.7".to_string();

    let mut invalid = CsvRow::new("錯誤設備", "機房A");
    invalid.property_no = "PC-BAD".to_string();
    invalid.ipv4 = "192.168.1.5".to_string();

    let (status, report) =
        post_csv(&pool, Some(false), build_csv(&[valid, invalid]).as_bytes()).await;
    assert_eq!(status, StatusCode::OK, "驗證報告仍為 200：{report}");
    assert_eq!(report["committed"], false);
    assert!(report["created"].is_null());
    assert_eq!(report["summary"]["errors"], 1);
    assert!(has_code(row_at(&report, 3), "ipv4_out_of_subnet"));

    // 全有全無：無任何殘留。
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM assets").await, 0);
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM interfaces").await, 0);
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM ip_assignments").await, 0);
}

#[tokio::test]
async fn external_change_after_preview_blocks_commit() {
    let pool = test_pool().await;
    let subnet_id = create_subnet(&pool, "10.0.0.0/24", None).await;

    let asset_id = create_asset(&pool, "既有設備", "機房A").await;
    let interface_id = create_interface(&pool, asset_id, json!({ "name": "eth0" })).await;

    let mut row = CsvRow::new("新設備", "機房B");
    row.property_no = "PC-RACE".to_string();
    row.ipv4 = "10.0.0.30".to_string();
    let csv = build_csv(&[row]);

    // 預覽當下無錯誤。
    let (status, preview) = post_csv(&pool, Some(true), csv.as_bytes()).await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    assert_eq!(preview["summary"]["errors"], 0);

    // 背後他人先指派同位址。
    assign_ip(&pool, subnet_id, "10.0.0.30", interface_id, "static").await;

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
        count(&pool, "SELECT COUNT(*) FROM assets").await,
        1,
        "僅既有資產"
    );
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM ip_assignments").await, 1);
    let owner: i64 = sqlx::query_scalar("SELECT interface_id FROM ip_assignments")
        .fetch_one(&pool)
        .await
        .expect("讀取指派");
    assert_eq!(owner, interface_id, "原指派未被異動");
    assert_eq!(
        count(
            &pool,
            "SELECT COUNT(*) FROM interfaces WHERE asset_id = 1 AND name = 'eth0'"
        )
        .await,
        1
    );
}
