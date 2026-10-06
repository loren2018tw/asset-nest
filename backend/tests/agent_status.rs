//! 觀測代理狀態整合測試（見票 01、spec §HTTP API、ADR-0019）。
//!
//! 覆蓋：心跳往返與 CIDR 精確對應、未對應仍可見、錯碼 401 與失敗累計、
//! 未設認證碼 503、在線門檻與排序、輸入驗證 400、網段刪除後代理保留。
//! 記憶體 SQLite＋`sqlx::migrate!`＋`tower::ServiceExt::oneshot`；連線來源
//! 以注入 `ConnectInfo` 模擬（比照 `peer_mac.rs`）。

use std::net::SocketAddr;
use std::path::PathBuf;

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Method, Request, StatusCode, header};
use chrono::{Duration, Utc};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use sqlx::SqlitePool;
use sqlx::sqlite::SqlitePoolOptions;
use tower::ServiceExt;

use asset_nest::observation::timestamp;
use asset_nest::{AppState, app};

/// 測試用共用認證碼。
const AUTH_CODE: &str = "agent-secret-01";
/// 測試連線來源（TEST-NET-3；忽略 XFF 後即 `source_ip`）。
const REMOTE: &str = "203.0.113.9:40123";

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

fn dist_dir() -> PathBuf {
    std::env::temp_dir().join("asset-nest-test-no-dist")
}

/// 建立已設認證碼與在線門檻的 AppState。
fn test_state(pool: &SqlitePool) -> AppState {
    AppState::new(pool.clone(), dist_dir())
        .with_agent_auth_code(Some(AUTH_CODE.to_string()))
        .with_agent_stale_secs(900)
}

/// 以 `oneshot` 發送請求（附連線來源）；回傳狀態碼與 JSON。
async fn send(
    state: &AppState,
    method: Method,
    uri: &str,
    body: Option<Value>,
    auth_code: Option<&str>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(code) = auth_code {
        builder = builder.header("x-auth-code", code);
    }
    let body = match body {
        Some(value) => {
            builder = builder.header(header::CONTENT_TYPE, "application/json");
            Body::from(value.to_string())
        }
        None => Body::empty(),
    };

    let mut request = builder.body(body).expect("建立請求");
    request
        .extensions_mut()
        .insert(ConnectInfo(REMOTE.parse::<SocketAddr>().expect("來源位址")));

    let response = app(state.clone()).oneshot(request).await.expect("執行請求");

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

/// 送出一筆心跳（帶測試認證碼）。
async fn heartbeat(state: &AppState, body: Value) -> (StatusCode, Value) {
    send(
        state,
        Method::POST,
        "/api/v1/agents/heartbeat",
        Some(body),
        Some(AUTH_CODE),
    )
    .await
}

/// 新增網段並斷言成功，回傳 id。
async fn create_subnet(state: &AppState, cidr: &str, name: &str) -> i64 {
    let (status, json) = send(
        state,
        Method::POST,
        "/api/v1/subnets",
        Some(json!({ "cidr": cidr, "name": name })),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "新增網段應成功：{json}");
    json["id"].as_i64().expect("回應含 id")
}

/// 讀取資料庫中的代理列數。
async fn agent_count(pool: &SqlitePool) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM agent")
        .fetch_one(pool)
        .await
        .expect("查詢代理列數")
}

#[tokio::test]
async fn heartbeat_matches_normalized_cidr_and_roundtrips() {
    let pool = test_pool().await;
    let state = test_state(&pool);
    let subnet_id = create_subnet(&state, "10.20.1.0/24", "代理區").await;

    // host bits 未收斂的回報值：正規化後精確對應受管網段
    let (status, response) = heartbeat(
        &state,
        json!({
            "instance_id": "b3f1a2c4-0000-4000-8000-000000000001",
            "name": "edge-agent-01",
            "version": "0.1.0",
            "subnet_cidr": "10.20.1.5/24",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "心跳應成功：{response}");
    assert_eq!(response["subnet_matched"], true);

    let (status, page) = send(&state, Method::GET, "/api/v1/agents", None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(page["stale_secs"], 900, "回應須附在線門檻");
    let items = page["items"].as_array().expect("items 為陣列");
    assert_eq!(items.len(), 1, "一筆心跳寫入一列代理");
    let agent = &items[0];
    assert_eq!(agent["instance_id"], "b3f1a2c4-0000-4000-8000-000000000001");
    assert_eq!(agent["name"], "edge-agent-01");
    assert_eq!(agent["version"], "0.1.0");
    assert_eq!(agent["source_ip"], "203.0.113.9", "來源 IP 取連線來源");
    assert_eq!(agent["subnet_cidr"], "10.20.1.0/24", "CIDR 須正規化入庫");
    assert_eq!(agent["subnet_id"], subnet_id);
    assert_eq!(agent["subnet_name"], "代理區");
    assert_eq!(agent["online"], true, "剛回報應在線");
    assert!(agent["first_report_at"].is_string());
    assert!(agent["last_report_at"].is_string());
    assert!(agent["last_observation_at"].is_null(), "本票不寫觀測時間");

    // 同 instance_id 再回報：upsert 不新增列、首次時間保留、欄位更新
    let first_report_at = agent["first_report_at"].as_str().expect("首次時間");
    let (status, response) = heartbeat(
        &state,
        json!({
            "instance_id": "b3f1a2c4-0000-4000-8000-000000000001",
            "name": "edge-agent-01",
            "version": "0.2.0",
            "subnet_cidr": "10.20.1.0/24",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "重複心跳應成功：{response}");
    assert_eq!(agent_count(&pool).await, 1, "同一代理不重複寫入");
    let (_, page) = send(&state, Method::GET, "/api/v1/agents", None, None).await;
    assert_eq!(page["items"][0]["version"], "0.2.0");
    assert_eq!(
        page["items"][0]["first_report_at"], first_report_at,
        "first_report_at 只寫首次"
    );
}

#[tokio::test]
async fn heartbeat_unmatched_cidr_still_visible() {
    let pool = test_pool().await;
    let state = test_state(&pool);

    let (status, response) = heartbeat(
        &state,
        json!({
            "instance_id": "b3f1a2c4-0000-4000-8000-000000000002",
            "name": "orphan-agent",
            "version": "0.1.0",
            "subnet_cidr": "10.99.0.0/24",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "未對應仍應入庫代理狀態：{response}");
    assert_eq!(response["subnet_matched"], false);

    let (status, page) = send(&state, Method::GET, "/api/v1/agents", None, None).await;
    assert_eq!(status, StatusCode::OK);
    let agent = &page["items"][0];
    assert!(agent["subnet_id"].is_null(), "對不到網段須留空");
    assert!(agent["subnet_name"].is_null());
    assert_eq!(agent["subnet_cidr"], "10.99.0.0/24", "仍保留回報值");
    assert_eq!(agent["online"], true, "未對應不影響在線判定");
}

#[tokio::test]
async fn wrong_auth_code_rejects_and_accumulates_without_touching_agent() {
    let pool = test_pool().await;
    let state = test_state(&pool);

    // 先有一筆成功心跳，作為「錯碼不得改寫」的對照
    let (status, _) = heartbeat(
        &state,
        json!({
            "instance_id": "b3f1a2c4-0000-4000-8000-000000000003",
            "name": "good-agent",
            "version": "0.1.0",
            "subnet_cidr": "10.30.0.0/24",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let before: String = sqlx::query_scalar("SELECT last_report_at FROM agent")
        .fetch_one(&pool)
        .await
        .expect("讀取心跳時間");

    // 錯碼兩次：401、不寫入代理、被拒回報累計
    for _ in 0..2 {
        let (status, body) = send(
            &state,
            Method::POST,
            "/api/v1/agents/heartbeat",
            Some(json!({
                "instance_id": "b3f1a2c4-0000-4000-8000-000000000004",
                "name": "ghost-agent",
                "version": "9.9.9",
                "subnet_cidr": "10.30.0.0/24",
            })),
            Some("wrong-code"),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "錯碼須拒收：{body}");
        assert_eq!(body["error"], "unauthorized");
    }

    assert_eq!(agent_count(&pool).await, 1, "錯碼不得新增代理");
    let after: String = sqlx::query_scalar("SELECT last_report_at FROM agent")
        .fetch_one(&pool)
        .await
        .expect("讀取心跳時間");
    assert_eq!(after, before, "錯碼不得更新既有代理");

    let (status, page) = send(
        &state,
        Method::GET,
        "/api/v1/agents/auth-failures",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let items = page["items"].as_array().expect("items 為陣列");
    assert_eq!(items.len(), 1, "同來源 IP 彙總為一列");
    let failure = &items[0];
    assert_eq!(failure["source_ip"], "203.0.113.9", "來源 IP 取連線來源");
    assert_eq!(
        failure["claimed_name"], "ghost-agent",
        "自報名稱 best-effort"
    );
    assert_eq!(failure["claimed_version"], "9.9.9");
    assert_eq!(failure["attempt_count"], 2, "兩次錯碼應累計");
    let first_attempt = failure["first_attempt_at"].as_str().expect("首次時間");
    let last_attempt = failure["last_attempt_at"].as_str().expect("最後時間");
    assert!(!first_attempt.is_empty());
    assert!(last_attempt >= first_attempt, "最後嘗試不得早於首次");
}

#[tokio::test]
async fn wrong_auth_code_with_broken_body_records_nulls() {
    let pool = test_pool().await;
    let state = test_state(&pool);

    // 空 body 無法解析：仍須 401 並記被拒回報（自報欄位留空）
    let (status, body) = send(
        &state,
        Method::POST,
        "/api/v1/agents/heartbeat",
        None,
        Some("wrong-code"),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "錯碼須拒收：{body}");

    let (status, page) = send(
        &state,
        Method::GET,
        "/api/v1/agents/auth-failures",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let failure = &page["items"][0];
    assert!(failure["claimed_name"].is_null(), "解析失敗留空");
    assert!(failure["claimed_version"].is_null());
    assert_eq!(failure["attempt_count"], 1);
}

#[tokio::test]
async fn missing_auth_code_returns_503_and_reads_still_work() {
    let pool = test_pool().await;
    let state = AppState::new(pool.clone(), dist_dir());

    let (status, body) = heartbeat(
        &state,
        json!({
            "instance_id": "b3f1a2c4-0000-4000-8000-000000000005",
            "name": "agent",
            "version": "0.1.0",
            "subnet_cidr": "10.0.0.0/24",
        }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::SERVICE_UNAVAILABLE,
        "未設碼須 503：{body}"
    );
    assert_eq!(body["error"], "service_unavailable");
    assert_eq!(agent_count(&pool).await, 0, "503 不得寫入代理");

    let failures: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM agent_auth_failure")
        .fetch_one(&pool)
        .await
        .expect("查詢被拒回報");
    assert_eq!(failures, 0, "503 不是認證失敗，不得累計");

    // 唯讀端點不受未設碼影響
    let (status, page) = send(&state, Method::GET, "/api/v1/agents", None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(page["stale_secs"], 900, "預設門檻");
    assert_eq!(page["items"].as_array().expect("items").len(), 0);

    let (status, page) = send(
        &state,
        Method::GET,
        "/api/v1/agents/auth-failures",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(page["items"].as_array().expect("items").len(), 0);
}

#[tokio::test]
async fn online_uses_stale_threshold_and_orders_by_last_report() {
    let pool = test_pool().await;
    let state = test_state(&pool);

    // 三台代理：時間直接改 DB 以取得可控差異（A 新、B 舊、C 最新）
    for (suffix, name) in [
        ("000000000006", "agent-a"),
        ("000000000007", "agent-b"),
        ("000000000008", "agent-c"),
    ] {
        let (status, _) = heartbeat(
            &state,
            json!({
                "instance_id": format!("b3f1a2c4-0000-4000-8000-{suffix}"),
                "name": name,
                "version": "0.1.0",
                "subnet_cidr": "10.40.0.0/24",
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
    }

    let now = Utc::now();
    for (suffix, older_secs) in [
        ("000000000006", 60),
        ("000000000007", 1_000),
        ("000000000008", 5),
    ] {
        sqlx::query("UPDATE agent SET last_report_at = ? WHERE id = ?")
            .bind(timestamp(now - Duration::seconds(older_secs)))
            .bind(format!("b3f1a2c4-0000-4000-8000-{suffix}"))
            .execute(&pool)
            .await
            .expect("調整心跳時間");
    }

    let (status, page) = send(&state, Method::GET, "/api/v1/agents", None, None).await;
    assert_eq!(status, StatusCode::OK);
    let items = page["items"].as_array().expect("items 為陣列");
    assert_eq!(items.len(), 3);
    let names: Vec<&str> = items
        .iter()
        .map(|item| item["name"].as_str().expect("name"))
        .collect();
    assert_eq!(
        names,
        vec!["agent-c", "agent-a", "agent-b"],
        "last_report_at 新到舊"
    );
    assert_eq!(items[0]["online"], true, "60 秒內在線");
    assert_eq!(items[1]["online"], true, "60 秒在 900 秒門檻內");
    assert_eq!(items[2]["online"], false, "1000 秒超過 900 秒門檻");

    // 換更嚴的門檻重讀：同一份資料改判離線，且回應反映門檻值
    let tight = state.clone().with_agent_stale_secs(30);
    let (status, page) = send(&tight, Method::GET, "/api/v1/agents", None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(page["stale_secs"], 30);
    let by_name: Vec<(&str, bool)> = page["items"]
        .as_array()
        .expect("items")
        .iter()
        .map(|item| {
            (
                item["name"].as_str().expect("name"),
                item["online"].as_bool().expect("online"),
            )
        })
        .collect();
    assert_eq!(
        by_name,
        vec![("agent-c", true), ("agent-a", false), ("agent-b", false)],
        "30 秒門檻下 60 秒前的回報即離線"
    );
}

#[tokio::test]
async fn heartbeat_validation_rejects_invalid_input() {
    let pool = test_pool().await;
    let state = test_state(&pool);

    let cases = [
        (json!({}), "instance_id", "缺漏 instance_id"),
        (
            json!({
                "instance_id": " ",
                "name": "n",
                "version": "v",
                "subnet_cidr": "10.0.0.0/24",
            }),
            "instance_id",
            "空字串 instance_id",
        ),
        (
            json!({
                "instance_id": "id-1",
                "name": "",
                "version": "v",
                "subnet_cidr": "10.0.0.0/24",
            }),
            "name",
            "空字串 name",
        ),
        (
            json!({
                "instance_id": "id-1",
                "name": "n",
                "version": "  ",
                "subnet_cidr": "10.0.0.0/24",
            }),
            "version",
            "空白 version",
        ),
        (
            json!({
                "instance_id": "id-1",
                "name": "n",
                "version": "v",
                "subnet_cidr": "not-a-cidr",
            }),
            "subnet_cidr",
            "非法 CIDR",
        ),
        (
            json!({
                "instance_id": "id-1",
                "name": "n",
                "version": "v",
                "subnet_cidr": "10.0.0.0/33",
            }),
            "subnet_cidr",
            "前綴長度超出範圍",
        ),
    ];

    for (body, field, label) in cases {
        let (status, response) = heartbeat(&state, body.clone()).await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "{label} 應回 400：{response}"
        );
        assert_eq!(response["error"], "validation_error", "{label}");
        assert_eq!(
            response["details"]["field"], field,
            "{label} 須標示欄位：{response}"
        );
    }

    assert_eq!(agent_count(&pool).await, 0, "驗證失敗不得寫入代理");
}

#[tokio::test]
async fn deleting_subnet_keeps_agent_and_clears_link() {
    let pool = test_pool().await;
    let state = test_state(&pool);
    let subnet_id = create_subnet(&state, "10.50.1.0/24", "待刪區").await;

    let (status, response) = heartbeat(
        &state,
        json!({
            "instance_id": "b3f1a2c4-0000-4000-8000-000000000009",
            "name": "agent-on-deleted-subnet",
            "version": "0.1.0",
            "subnet_cidr": "10.50.1.0/24",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{response}");
    assert_eq!(response["subnet_matched"], true);

    // 刪除網段：代理仍可見，subnet_id 依 FK 設空（ON DELETE SET NULL）
    let (status, _) = send(
        &state,
        Method::DELETE,
        &format!("/api/v1/subnets/{subnet_id}"),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "空網段應可刪除");

    let (status, page) = send(&state, Method::GET, "/api/v1/agents", None, None).await;
    assert_eq!(status, StatusCode::OK);
    let agent = &page["items"][0];
    assert!(agent["subnet_id"].is_null(), "網段刪除後須設空");
    assert!(agent["subnet_name"].is_null());
    assert_eq!(agent["online"], true, "代理狀態不受網段刪除影響");
}
