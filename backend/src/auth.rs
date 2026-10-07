//! 認證核心：單一帳號的帳密比對、簽章工作階段與登入 middleware
//! （見 `docs/adr/0021`、spec §3.2／§3.3）。
//!
//! 工作階段為無狀態 token：`base64url(claims).base64url(hmac-sha256)`。
//! 金鑰由帳密推導，帳密任一變更即使所有既有工作階段失效。
//! API 端點（session／login／logout）見 `crate::api` 的 auth 模組。

use axum::extract::{Request, State};
use axum::http::Method;
use axum::middleware::Next;
use axum::response::Response;
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::{DateTime, Duration, Utc};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use crate::AppState;
use crate::api::ApiError;

/// 工作階段 cookie 名稱。
pub const SESSION_COOKIE_NAME: &str = "asset_nest_session";
/// 工作階段有效天數；固定、不隨活動延長（見 ADR-0021）。
pub const SESSION_DAYS: i64 = 30;
/// 工作階段 cookie 的 `Max-Age`（秒）＝ [`SESSION_DAYS`] 天。
const SESSION_MAX_AGE_SECS: i64 = SESSION_DAYS * 24 * 60 * 60;
/// token 版本前綴；claims 格式為 `v1|{expiry_unix}|{username}`。
const TOKEN_VERSION: &str = "v1";
/// 簽章金鑰的用途前綴（改版時遞增，使舊 token 全數失效）。
const KEY_PREFIX: &str = "asset-nest-session-v1";

/// 登入帳密（來自 `AUTH_USERNAME`／`AUTH_PASSWORD`；見 [`crate::config::Config`]）。
#[derive(Debug, Clone)]
pub struct AuthConfig {
    pub username: String,
    pub password: String,
}

/// 簽發工作階段 token：claims `v1|{expiry_unix}|{username}`，
/// `expiry = now + 30 天`（UTC unix 秒）。
pub fn sign_token(config: &AuthConfig, now: DateTime<Utc>) -> String {
    let expiry = now + Duration::days(SESSION_DAYS);
    let claims = format!("{TOKEN_VERSION}|{}|{}", expiry.timestamp(), config.username);

    let mut mac = session_mac(config);
    mac.update(claims.as_bytes());
    let signature = mac.finalize().into_bytes();

    format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(claims.as_bytes()),
        URL_SAFE_NO_PAD.encode(signature)
    )
}

/// 驗證工作階段 token；成功回傳 claims 中的 username，任一不符回傳 `None`。
///
/// 檢查：解碼、簽章常數時間比對（[`Mac::verify_slice`]）、未過期
/// （`now < expiry`）、claims username 與現行設定相符。
pub fn verify_token(config: &AuthConfig, token: &str, now: DateTime<Utc>) -> Option<String> {
    let (claims_b64, signature_b64) = token.split_once('.')?;
    let signature = URL_SAFE_NO_PAD.decode(signature_b64).ok()?;
    let claims = String::from_utf8(URL_SAFE_NO_PAD.decode(claims_b64).ok()?).ok()?;

    let mut mac = session_mac(config);
    mac.update(claims.as_bytes());
    mac.verify_slice(&signature).ok()?;

    let mut parts = claims.splitn(3, '|');
    let version = parts.next()?;
    let expiry = parts.next()?.parse::<i64>().ok()?;
    let username = parts.next()?;
    if version != TOKEN_VERSION || now.timestamp() >= expiry || username != config.username {
        return None;
    }
    Some(username.to_string())
}

/// 常數時間比對帳密（帳號與密碼皆比；見 ADR-0021）。
pub fn credentials_match(config: &AuthConfig, username: &str, password: &str) -> bool {
    let username_matches = config.username.as_bytes().ct_eq(username.as_bytes());
    let password_matches = config.password.as_bytes().ct_eq(password.as_bytes());
    bool::from(username_matches & password_matches)
}

/// 組出登入用的工作階段 cookie（`HttpOnly`、`SameSite=Lax`、`Path=/`、
/// `Max-Age=2592000`）。不設 `Secure`——傳輸安全由部署層提供（見 ADR-0022）。
pub fn session_cookie(token: &str) -> Cookie<'static> {
    let mut cookie = Cookie::new(SESSION_COOKIE_NAME, token.to_string());
    cookie.set_http_only(true);
    cookie.set_same_site(SameSite::Lax);
    cookie.set_path("/");
    cookie.set_max_age(time::Duration::seconds(SESSION_MAX_AGE_SECS));
    cookie
}

/// 組出登出用的清除 cookie：同名同屬性、`Max-Age=0`、值為空。
pub fn cleared_cookie() -> Cookie<'static> {
    let mut cookie = Cookie::new(SESSION_COOKIE_NAME, String::new());
    cookie.set_http_only(true);
    cookie.set_same_site(SameSite::Lax);
    cookie.set_path("/");
    cookie.set_max_age(time::Duration::seconds(0));
    cookie
}

/// 登入 middleware：驗證 `asset_nest_session` cookie（見 spec §3.3）。
///
/// - `state.auth` 為 `None`（測試路徑）→ 全數放行。
/// - 免登入白名單（method＋path 完全相符）→ 放行。
/// - 其餘：cookie 缺失／無效／逾期 → 401「請先登入」（JSON）。
pub async fn require_login(
    State(state): State<AppState>,
    jar: CookieJar,
    request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    // 未附掛認證：不擋任何請求（`AppState::new()` 的測試路徑）。
    let Some(config) = state.auth.as_ref() else {
        return Ok(next.run(request).await);
    };

    if is_public_endpoint(request.method(), request.uri().path()) {
        return Ok(next.run(request).await);
    }

    let authenticated = jar
        .get(SESSION_COOKIE_NAME)
        .and_then(|cookie| verify_token(config, cookie.value(), Utc::now()))
        .is_some();
    if authenticated {
        return Ok(next.run(request).await);
    }

    Err(ApiError::unauthorized("請先登入"))
}

/// 免登入端點白名單：method＋path 完全相符才放行（見 spec §3.3）。
fn is_public_endpoint(method: &Method, path: &str) -> bool {
    (method == Method::GET && path == "/api/health")
        || (method == Method::POST
            && matches!(
                path,
                "/api/v1/login"
                    | "/api/v1/logout"
                    | "/api/v1/agents/heartbeat"
                    | "/api/v1/agents/observations"
            ))
}

/// 由帳密推導 HMAC-SHA256 金鑰：
/// `SHA-256("asset-nest-session-v1" ‖ 0x00 ‖ username ‖ 0x00 ‖ password)`。
fn session_mac(config: &AuthConfig) -> Hmac<Sha256> {
    let mut hasher = Sha256::new();
    hasher.update(KEY_PREFIX.as_bytes());
    hasher.update([0u8]);
    hasher.update(config.username.as_bytes());
    hasher.update([0u8]);
    hasher.update(config.password.as_bytes());
    let key = hasher.finalize();

    Hmac::<Sha256>::new_from_slice(&key).expect("HMAC 接受任意長度金鑰")
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    /// 測試用固定「現在」（2026-10-07 12:00:00 UTC）。
    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 7, 12, 0, 0).unwrap()
    }

    fn config() -> AuthConfig {
        AuthConfig {
            username: "alice".to_string(),
            password: "s3cret".to_string(),
        }
    }

    /// 以 `config` 的金鑰為任意 claims 造出簽章有效的 token（驗證分支測試用）。
    fn forge_token(config: &AuthConfig, claims: &str) -> String {
        let mut mac = session_mac(config);
        mac.update(claims.as_bytes());
        let signature = mac.finalize().into_bytes();
        format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(claims.as_bytes()),
            URL_SAFE_NO_PAD.encode(signature)
        )
    }

    /// 將字串最後一個字元換成不同的 base64 字元（竄改簽章測試用）。
    fn flip_last_char(value: &str) -> String {
        let mut chars: Vec<char> = value.chars().collect();
        let last = chars.last_mut().expect("非空字串");
        *last = if *last == 'A' { 'B' } else { 'A' };
        chars.into_iter().collect()
    }

    #[test]
    fn session_length_is_thirty_days() {
        assert_eq!(SESSION_DAYS, 30);
        assert_eq!(SESSION_MAX_AGE_SECS, 2_592_000);
    }

    #[test]
    fn sign_and_verify_round_trip() {
        let config = config();
        let token = sign_token(&config, now());

        assert_eq!(
            verify_token(&config, &token, now()).as_deref(),
            Some("alice"),
            "簽章往返應帶回 username"
        );
    }

    #[test]
    fn token_is_valid_until_expiry_and_invalid_at_expiry() {
        let config = config();
        let token = sign_token(&config, now());
        let just_before = now() + Duration::days(SESSION_DAYS) - Duration::seconds(1);
        let at_expiry = now() + Duration::days(SESSION_DAYS);

        assert_eq!(
            verify_token(&config, &token, just_before).as_deref(),
            Some("alice"),
            "到期前仍有效"
        );
        assert_eq!(
            verify_token(&config, &token, at_expiry),
            None,
            "期滿瞬間即失效（now < expiry）"
        );
    }

    #[test]
    fn token_signed_in_the_past_is_expired_now() {
        let config = config();
        let issued = now() - Duration::days(SESSION_DAYS + 1);
        let token = sign_token(&config, issued);

        assert_eq!(
            verify_token(&config, &token, now()),
            None,
            "以過去的 now 簽即已過期"
        );
    }

    #[test]
    fn token_round_trips_username_containing_pipe() {
        let config = AuthConfig {
            username: "a|b".to_string(),
            password: "pw".to_string(),
        };
        let token = sign_token(&config, now());

        assert_eq!(
            verify_token(&config, &token, now()).as_deref(),
            Some("a|b"),
            "username 允許含 |（splitn(3)）"
        );
    }

    #[test]
    fn tampered_claims_are_rejected() {
        let config = config();
        let token = sign_token(&config, now());
        let signature = token.split_once('.').expect("含分隔點").1;

        // 沿用原簽章、換上偽造 claims（把期限延長到 60 天）。
        let forged_claims = URL_SAFE_NO_PAD.encode(format!(
            "v1|{}|alice",
            (now() + Duration::days(60)).timestamp()
        ));
        assert_eq!(
            verify_token(&config, &format!("{forged_claims}.{signature}"), now()),
            None
        );
    }

    #[test]
    fn tampered_signature_is_rejected() {
        let config = config();
        let token = sign_token(&config, now());
        let (claims, signature) = token.split_once('.').expect("含分隔點");

        let forged = format!("{claims}.{}", flip_last_char(signature));
        assert_eq!(verify_token(&config, &forged, now()), None);
    }

    #[test]
    fn changing_credentials_invalidates_tokens() {
        let config = config();
        let token = sign_token(&config, now());

        let other_password = AuthConfig {
            password: "different".to_string(),
            ..config.clone()
        };
        assert_eq!(
            verify_token(&other_password, &token, now()),
            None,
            "改密碼即失效"
        );

        let other_username = AuthConfig {
            username: "bob".to_string(),
            ..config.clone()
        };
        assert_eq!(
            verify_token(&other_username, &token, now()),
            None,
            "改帳號即失效"
        );
    }

    #[test]
    fn claims_username_must_match_config() {
        let config = config();
        let claims = format!("v1|{}|bob", (now() + Duration::days(1)).timestamp());
        let token = forge_token(&config, &claims);

        assert_eq!(
            verify_token(&config, &token, now()),
            None,
            "簽章有效但 claims username 不符仍拒絕"
        );
    }

    #[test]
    fn wrong_version_is_rejected_even_with_valid_signature() {
        let config = config();
        let claims = format!("v2|{}|alice", (now() + Duration::days(1)).timestamp());
        let token = forge_token(&config, &claims);

        assert_eq!(verify_token(&config, &token, now()), None, "版本須為 v1");
    }

    #[test]
    fn malformed_tokens_are_rejected() {
        let config = config();
        let valid = sign_token(&config, now());
        let (claims, signature) = valid.split_once('.').expect("含分隔點");

        for malformed in [
            "",
            "no-separator",
            ".onlysignature",
            "!!!.???",  // 兩段皆非 base64
            "!!!.MTIz", // claims 非 base64
            "MTIz.???", // 簽章非 base64
            "not-base64-token",
        ] {
            assert_eq!(
                verify_token(&config, malformed, now()),
                None,
                "應拒絕 {malformed:?}"
            );
        }

        assert_eq!(
            verify_token(&config, &format!("{claims}."), now()),
            None,
            "簽章為空應拒絕"
        );
        assert_eq!(
            verify_token(&config, &format!(".{signature}"), now()),
            None,
            "claims 為空應拒絕"
        );
    }

    #[test]
    fn credentials_match_requires_both_fields() {
        let config = config();

        assert!(credentials_match(&config, "alice", "s3cret"), "正確帳密");
        assert!(!credentials_match(&config, "alice", "wrong"), "錯密碼");
        assert!(!credentials_match(&config, "bob", "s3cret"), "錯帳號");
        assert!(!credentials_match(&config, "bob", "wrong"), "皆錯");
        assert!(!credentials_match(&config, "", ""), "空帳密");
    }

    #[test]
    fn session_cookie_has_expected_attributes() {
        let cookie = session_cookie("tok");

        assert_eq!(cookie.name(), SESSION_COOKIE_NAME);
        assert_eq!(cookie.value(), "tok");
        assert_eq!(cookie.http_only(), Some(true));
        assert_eq!(cookie.same_site(), Some(SameSite::Lax));
        assert_eq!(cookie.path(), Some("/"));
        assert_eq!(
            cookie.max_age(),
            Some(time::Duration::seconds(2_592_000)),
            "Max-Age 為 30 天"
        );
        assert_ne!(cookie.secure(), Some(true), "不設 Secure（見 ADR-0022）");
    }

    #[test]
    fn cleared_cookie_expires_immediately() {
        let cookie = cleared_cookie();

        assert_eq!(cookie.name(), SESSION_COOKIE_NAME);
        assert_eq!(cookie.value(), "");
        assert_eq!(cookie.max_age(), Some(time::Duration::seconds(0)));
        assert_eq!(cookie.path(), Some("/"));
        assert_eq!(cookie.http_only(), Some(true));
    }

    #[test]
    fn whitelist_requires_exact_method_and_path() {
        assert!(is_public_endpoint(&Method::GET, "/api/health"));
        assert!(is_public_endpoint(&Method::POST, "/api/v1/login"));
        assert!(is_public_endpoint(&Method::POST, "/api/v1/logout"));
        assert!(is_public_endpoint(
            &Method::POST,
            "/api/v1/agents/heartbeat"
        ));
        assert!(is_public_endpoint(
            &Method::POST,
            "/api/v1/agents/observations"
        ));

        assert!(
            !is_public_endpoint(&Method::GET, "/api/v1/login"),
            "method 須完全相符"
        );
        assert!(!is_public_endpoint(&Method::POST, "/api/health"));
        assert!(!is_public_endpoint(&Method::POST, "/api/v1/session"));
        assert!(!is_public_endpoint(
            &Method::GET,
            "/api/v1/agents/heartbeat"
        ));
        assert!(
            !is_public_endpoint(&Method::GET, "/api/v1/agents/heartbeat/"),
            "path 須完全相符"
        );
    }
}
