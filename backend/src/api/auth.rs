//! `/api/v1` 登入／登出／工作階段路由（見 spec §4、ADR-0021）。
//!
//! `POST /login` 與 `POST /logout` 位於免登入白名單
//! （見 [`crate::auth::require_login`]），其餘端點由登入 middleware 保護。

use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use axum_extra::extract::cookie::CookieJar;
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::AppState;
use crate::api::ApiError;
use crate::auth;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/session", get(session))
        .route("/login", post(login))
        .route("/logout", post(logout))
}

/// 登入／工作階段回應：目前登入帳號。
#[derive(Debug, Serialize)]
struct SessionResponse {
    username: String,
}

/// 登入輸入：`{username, password}`；缺欄位或非 JSON 由 `JsonRejection` 轉 400。
#[derive(Debug, Deserialize)]
struct LoginInput {
    username: String,
    password: String,
}

/// `GET /api/v1/session`：已登入（經 middleware 驗證）回傳設定帳號。
async fn session(State(state): State<AppState>) -> Result<Json<SessionResponse>, ApiError> {
    match state.auth.as_ref() {
        Some(config) => Ok(Json(SessionResponse {
            username: config.username.clone(),
        })),
        // 未附掛認證時 middleware 全數放行，此處視同未登入。
        None => Err(ApiError::unauthorized("請先登入")),
    }
}

/// `POST /api/v1/login`：帳密比對成功 → 200＋session cookie
/// （`Set-Cookie`）；失敗一律 401「帳號或密碼錯誤」。
async fn login(
    State(state): State<AppState>,
    payload: Result<Json<LoginInput>, JsonRejection>,
) -> Result<Response, ApiError> {
    let Json(input) = payload.map_err(|_| ApiError::validation("請求內容格式錯誤"))?;

    let Some(config) = state.auth.as_ref() else {
        return Err(ApiError::unauthorized("帳號或密碼錯誤"));
    };

    if !auth::credentials_match(config, &input.username, &input.password) {
        return Err(ApiError::unauthorized("帳號或密碼錯誤"));
    }

    let token = auth::sign_token(config, Utc::now());
    let jar = CookieJar::new().add(auth::session_cookie(&token));

    Ok((
        jar,
        Json(SessionResponse {
            username: config.username.clone(),
        }),
    )
        .into_response())
}

/// `POST /api/v1/logout`：免登入、冪等；一律 204＋清除 cookie。
async fn logout() -> impl IntoResponse {
    (
        StatusCode::NO_CONTENT,
        CookieJar::new().add(auth::cleared_cookie()),
    )
}
