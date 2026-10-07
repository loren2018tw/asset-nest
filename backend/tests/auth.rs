//! 登入認證整合測試（見票 02、spec §3.3／§4、ADR-0021）。
//!
//! 覆蓋：登入成功與 `Set-Cookie` 屬性、帳密錯（固定訊息）、缺欄位／壞 JSON、
//! `/session` 未登入與已登入、logout 冪等與清除 cookie、受保護端點與未知
//! `/api` 路徑、免登入白名單（health 與代理端點維持既有語意）、逾期 token
//! 與無效 cookie。記憶體 SQLite＋`sqlx::migrate!`＋
//! `tower::ServiceExt::oneshot`；連線來源以注入 `ConnectInfo` 模擬
//! （比照 `agent_ingest.rs`）。

use std::net::SocketAddr;
use std::path::PathBuf;

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{HeaderMap, Method, Request, StatusCode, header};
use chrono::{Duration, Utc};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use sqlx::SqlitePool;
use sqlx::sqlite::SqlitePoolOptions;
use tower::ServiceExt;

use asset_nest::auth::{AuthConfig, SESSION_COOKIE_NAME, sign_token};
use asset_nest::{AppState, app};

/// 測試帳密。
const USERNAME: &str = "admin";
const PASSWORD: &str = "secret-01";
/// 測試用代理認證碼（白名單語意測試）。
const AUTH_CODE: &str = "agent-secret-02";
/// 測試連線來源（代理端點需要；忽略 XFF 後即 `source_ip`）。
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

/// 建立已附掛登入認證的 `AppState`。
fn test_state(pool: &SqlitePool) -> AppState {
    AppState::new(pool.clone(), dist_dir()).with_auth(USERNAME, PASSWORD)
}

/// 測試帳密的認證設定（簽逾期 token 用）。
fn auth_config() -> AuthConfig {
    AuthConfig {
        username: USERNAME.to_string(),
        password: PASSWORD.to_string(),
    }
}

/// 發送 `oneshot` 請求並回傳（狀態、標頭、JSON body；空 body 為 `Null`）。
async fn run(state: &AppState, mut request: Request<Body>) -> (StatusCode, HeaderMap, Value) {
    request
        .extensions_mut()
        .insert(ConnectInfo(REMOTE.parse::<SocketAddr>().expect("來源位址")));

    let response = app(state.clone()).oneshot(request).await.expect("執行請求");
    let status = response.status();
    let headers = response.headers().clone();
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

    (status, headers, json)
}

/// 以可選 JSON body 與可選 session token 發送請求。
async fn send(
    state: &AppState,
    method: Method,
    uri: &str,
    body: Option<Value>,
    token: Option<&str>,
) -> (StatusCode, HeaderMap, Value) {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(token) = token {
        builder = builder.header(header::COOKIE, format!("{SESSION_COOKIE_NAME}={token}"));
    }
    let body = match body {
        Some(value) => {
            builder = builder.header(header::CONTENT_TYPE, "application/json");
            Body::from(value.to_string())
        }
        None => Body::empty(),
    };

    run(state, builder.body(body).expect("建立請求")).await
}

/// 以原樣 body 發送請求（壞 JSON 測試用）。
async fn send_raw(
    state: &AppState,
    method: Method,
    uri: &str,
    raw: &str,
    token: Option<&str>,
) -> (StatusCode, HeaderMap, Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(token) = token {
        builder = builder.header(header::COOKIE, format!("{SESSION_COOKIE_NAME}={token}"));
    }

    run(
        state,
        builder.body(Body::from(raw.to_string())).expect("建立請求"),
    )
    .await
}

/// 送出登入請求；回傳（狀態、`Set-Cookie` 值、JSON）。
async fn login(
    state: &AppState,
    username: &str,
    password: &str,
) -> (StatusCode, Option<String>, Value) {
    let (status, headers, json) = send(
        state,
        Method::POST,
        "/api/v1/login",
        Some(json!({ "username": username, "password": password })),
        None,
    )
    .await;

    let set_cookie = headers
        .get(header::SET_COOKIE)
        .map(|value| value.to_str().expect("Set-Cookie 為 ASCII").to_string());

    (status, set_cookie, json)
}

/// 從 `Set-Cookie` 標頭取出 session token 值。
fn session_token(headers: &HeaderMap) -> String {
    let set_cookie = headers
        .get(header::SET_COOKIE)
        .expect("回應含 Set-Cookie")
        .to_str()
        .expect("Set-Cookie 為 ASCII");
    let (name_value, _) = set_cookie.split_once(';').expect("Set-Cookie 含屬性");
    let (name, token) = name_value.split_once('=').expect("cookie 為 name=value");
    assert_eq!(name, SESSION_COOKIE_NAME, "cookie 名稱");

    token.to_string()
}

/// 登入並斷言成功，回傳 session token（後續請求帶 cookie 用）。
async fn login_token(state: &AppState) -> String {
    let (status, headers, json) = send(
        state,
        Method::POST,
        "/api/v1/login",
        Some(json!({ "username": USERNAME, "password": PASSWORD })),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "登入應成功：{json}");

    session_token(&headers)
}

#[tokio::test]
async fn login_success_sets_session_cookie_with_expected_attributes() {
    let pool = test_pool().await;
    let state = test_state(&pool);

    let (status, set_cookie, json) = login(&state, USERNAME, PASSWORD).await;

    assert_eq!(status, StatusCode::OK, "登入成功：{json}");
    assert_eq!(json["username"], USERNAME);

    let set_cookie = set_cookie.expect("登入成功須 Set-Cookie");
    assert!(
        set_cookie.starts_with("asset_nest_session="),
        "{set_cookie}"
    );
    assert!(set_cookie.contains("HttpOnly"), "{set_cookie}");
    assert!(set_cookie.contains("SameSite=Lax"), "{set_cookie}");
    assert!(set_cookie.contains("Path=/"), "{set_cookie}");
    assert!(set_cookie.contains("Max-Age=2592000"), "{set_cookie}");
    assert!(!set_cookie.contains("Secure"), "不設 Secure：{set_cookie}");
}

#[tokio::test]
async fn wrong_credentials_return_generic_401() {
    let pool = test_pool().await;
    let state = test_state(&pool);

    for (username, password) in [
        (USERNAME, "wrong-password"),
        ("other-user", PASSWORD),
        ("other-user", "wrong-password"),
    ] {
        let (status, set_cookie, json) = login(&state, username, password).await;

        assert_eq!(status, StatusCode::UNAUTHORIZED, "帳密錯須 401：{json}");
        assert_eq!(json["error"], "unauthorized");
        assert_eq!(json["message"], "帳號或密碼錯誤");
        assert!(set_cookie.is_none(), "失敗不應 Set-Cookie");
    }
}

#[tokio::test]
async fn missing_fields_or_malformed_json_return_400() {
    let pool = test_pool().await;
    let state = test_state(&pool);

    for body in [
        json!({}),
        json!({ "username": USERNAME }),
        json!({ "password": PASSWORD }),
    ] {
        let (status, _, json) = send(&state, Method::POST, "/api/v1/login", Some(body), None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "缺欄位須 400：{json}");
    }

    let (status, _, json) =
        send_raw(&state, Method::POST, "/api/v1/login", "{not json", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "壞 JSON 須 400：{json}");
}

#[tokio::test]
async fn session_requires_login_and_returns_username_after_login() {
    let pool = test_pool().await;
    let state = test_state(&pool);

    let (status, _, json) = send(&state, Method::GET, "/api/v1/session", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "未登入須 401：{json}");
    assert_eq!(json["message"], "請先登入");

    let token = login_token(&state).await;
    let (status, _, json) = send(&state, Method::GET, "/api/v1/session", None, Some(&token)).await;
    assert_eq!(status, StatusCode::OK, "已登入應 200：{json}");
    assert_eq!(json["username"], USERNAME);
}

#[tokio::test]
async fn logout_is_idempotent_and_clears_cookie() {
    let pool = test_pool().await;
    let state = test_state(&pool);

    let token = login_token(&state).await;

    let (status, headers, _) =
        send(&state, Method::POST, "/api/v1/logout", None, Some(&token)).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "登出須 204");

    let set_cookie = headers
        .get(header::SET_COOKIE)
        .expect("登出須 Set-Cookie")
        .to_str()
        .expect("Set-Cookie 為 ASCII");
    assert!(
        set_cookie.starts_with("asset_nest_session=;"),
        "清除 cookie：{set_cookie}"
    );
    assert!(set_cookie.contains("Max-Age=0"), "立即過期：{set_cookie}");
    assert!(set_cookie.contains("HttpOnly"), "{set_cookie}");

    let (status, _, _) = send(&state, Method::POST, "/api/v1/logout", None, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "再次登出仍 204（冪等）");
}

#[tokio::test]
async fn protected_endpoint_requires_login() {
    let pool = test_pool().await;
    let state = test_state(&pool);

    let (status, _, json) = send(&state, Method::GET, "/api/v1/assets", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "未登入須 401：{json}");
    assert_eq!(json["message"], "請先登入");

    let token = login_token(&state).await;
    let (status, _, json) = send(&state, Method::GET, "/api/v1/assets", None, Some(&token)).await;
    assert_eq!(status, StatusCode::OK, "已登入應 200：{json}");
}

#[tokio::test]
async fn unknown_api_path_requires_login_then_returns_404() {
    let pool = test_pool().await;
    let state = test_state(&pool);

    let (status, _, json) = send(&state, Method::GET, "/api/v1/nope", None, None).await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "未登入未匹配路徑須 401：{json}"
    );
    assert_eq!(json["message"], "請先登入");

    let token = login_token(&state).await;
    let (status, _, json) = send(&state, Method::GET, "/api/v1/nope", None, Some(&token)).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "登入後未匹配應 404：{json}");
    assert_eq!(json["error"], "not_found");
}

#[tokio::test]
async fn whitelist_health_is_public() {
    let pool = test_pool().await;
    let state = test_state(&pool);

    let (status, _, json) = send(&state, Method::GET, "/api/health", None, None).await;
    assert_eq!(status, StatusCode::OK, "health 免登入：{json}");
    assert_eq!(json["status"], "ok");
}

#[tokio::test]
async fn whitelist_agent_endpoints_keep_agent_semantics() {
    let pool = test_pool().await;

    // 未設 AGENT_AUTH_CODE：既有語意 503，不得被登入 middleware 擋成 401。
    let state = test_state(&pool);
    let (status, _, json) = send(
        &state,
        Method::POST,
        "/api/v1/agents/heartbeat",
        Some(json!({})),
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::SERVICE_UNAVAILABLE,
        "未設碼回 503：{json}"
    );
    assert_eq!(json["error"], "service_unavailable");

    // 已設碼但未帶：既有語意 401「代理認證碼不符」，非登入 401。
    let state = test_state(&pool).with_agent_auth_code(Some(AUTH_CODE.to_string()));
    let (status, _, json) = send(
        &state,
        Method::POST,
        "/api/v1/agents/heartbeat",
        Some(json!({})),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "認證碼不符回 401：{json}");
    assert_eq!(json["message"], "代理認證碼不符", "不得回登入訊息");
}

#[tokio::test]
async fn expired_token_is_rejected() {
    let pool = test_pool().await;
    let state = test_state(&pool);

    // 以 31 天前的 now 簽發：30 天期已過（見 `sign_token`）。
    let expired = sign_token(&auth_config(), Utc::now() - Duration::days(31));
    let (status, _, json) =
        send(&state, Method::GET, "/api/v1/session", None, Some(&expired)).await;

    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "逾期 token 須 401：{json}"
    );
    assert_eq!(json["message"], "請先登入");
}

#[tokio::test]
async fn garbage_cookie_is_rejected() {
    let pool = test_pool().await;
    let state = test_state(&pool);

    let (status, _, json) = send(
        &state,
        Method::GET,
        "/api/v1/session",
        None,
        Some("not-a-token"),
    )
    .await;

    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "無效 cookie 須 401：{json}"
    );
    assert_eq!(json["message"], "請先登入");
}

#[tokio::test]
async fn login_requires_post_method() {
    let pool = test_pool().await;
    let state = test_state(&pool);

    let (status, _, json) = send(&state, Method::GET, "/api/v1/login", None, None).await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "GET /api/v1/login 非白名單須 401：{json}"
    );
    assert_eq!(json["message"], "請先登入");
}
