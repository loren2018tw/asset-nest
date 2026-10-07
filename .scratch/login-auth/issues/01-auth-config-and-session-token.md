# 01 — 後端：認證設定與 session 簽章核心

Status: done
Blocked by: —

## 目標

實作 `spec §3.1、§3.2`：`AUTH_USERNAME`／`AUTH_PASSWORD` 設定與簽章 token 核心。本票不做 middleware、API 端點與 AppState 掛載（票 02），也不改任何既有 API 行為。

## 範圍

- `backend/Cargo.toml` 新增依賴：`hmac`、`sha2`、`base64`、`subtle`、`axum-extra`（`cookie` feature；版本以 axum 0.8 相容為準）。
- `backend/src/config.rs`：
  - `Config` 新增 `auth_username: String`、`auth_password: String`。
  - 常數 `DEFAULT_AUTH_USERNAME`／`DEFAULT_AUTH_PASSWORD`＝`"admin"`；解析函式（例：`parse_auth_field(raw: Option<String>, default: &str) -> String`）：trim；空／缺 → 預設。
  - `from_env` 讀取 `AUTH_USERNAME`／`AUTH_PASSWORD`。
  - 單元測試：未設定、空字串、全空白 → 預設；前後空白 trim；非空原樣。
- `backend/src/auth.rs` 改寫（移除 noop 敘述與 `noop_auth`）：
  - `pub struct AuthConfig { pub username: String, pub password: String }`。
  - `pub fn sign_token(config: &AuthConfig, now: DateTime<Utc>) -> String`：claims `v1|{expiry_unix}|{username}`（expiry＝now＋30 天）、金鑰 `SHA-256("asset-nest-session-v1"‖0x00‖username‖0x00‖password)`、token `base64url(claims).base64url(hmac-sha256)`。
  - `pub fn verify_token(config: &AuthConfig, token: &str, now: DateTime<Utc>) -> Option<String>`（成功回 username）：解碼、簽章常數時間比對（`Mac::verify_slice`）、`now < expiry`、claims username 與設定相符。
  - `pub fn credentials_match(config: &AuthConfig, username: &str, password: &str) -> bool`：常數時間比對（`subtle` 或等效），帳號與密碼皆比。
  - Cookie：`pub fn session_cookie(token: &str) -> Cookie<'static>`（`asset_nest_session`、HttpOnly、SameSite=Lax、Path=/、Max-Age=2592000、**不設 Secure**）、`pub fn cleared_cookie() -> Cookie<'static>`（Max-Age=0）。
  - 常數：cookie 名、`SESSION_DAYS = 30`。
  - 單元測試（注入固定 `now`）：簽章往返；過期（以過去的 now 簽、現在驗 → None）；竄改 claims／簽章任一 → None；改 username 或 password → None；username 含 `|` 往返正常；空 token／缺分隔點／壞 base64 → None；帳密比對（正確、錯帳號、錯密碼）；cookie 屬性（HttpOnly／Lax／Path／Max-Age，無 Secure）。

## 驗收

- `cargo test --manifest-path backend/Cargo.toml --lib config`、`--lib auth` 全綠。
- `pnpm test` 全綠（既有行為不變）；`cargo fmt --manifest-path backend/Cargo.toml -- --check`。

## 注意

- 風格比照現有模組：中文 doc comment；時間以參數注入（比照 `is_expired` 的 today 注入）。
- `lib.rs` 的 `pub mod auth;` 保留；本票不接線（票 02）。
- 不要 `git commit`；不要動 `.scratch/` 內其他票。

## Comments

實作完成（2026-10-07）：

- 變更摘要：
  - `backend/Cargo.toml`：新增 `hmac = "0.12"`、`sha2 = "0.10"`、`base64 = "0.22"`、`subtle = "2"`、`axum-extra = { version = "0.10", features = ["cookie"] }`、`time = "0.3"`。
  - `backend/src/config.rs`：新增 `DEFAULT_AUTH_USERNAME`／`DEFAULT_AUTH_PASSWORD`（皆 `"admin"`，`pub`）與 `Config.auth_username`／`auth_password`；`parse_auth_field(raw, default)`（trim；空／缺 → 預設）；`from_env` 讀 `AUTH_USERNAME`／`AUTH_PASSWORD`。單元測試涵蓋未設定／空字串／全空白／trim／非空原樣／常數值。
  - `backend/src/auth.rs` 改寫：`AuthConfig { username, password }`；`sign_token`（claims `v1|{expiry_unix}|{username}`、`expiry = now + 30 天`、金鑰 `SHA-256(prefix ‖ 0x00 ‖ username ‖ 0x00 ‖ password)`、`base64url(claims).base64url(hmac-sha256)`）；`verify_token`（`split_once('.')`、base64 解碼、`Mac::verify_slice` 常數時間比對、`now < expiry`、claims username 相符）；`credentials_match`（`subtle` 常數時間、帳密皆比）；`session_cookie`（`asset_nest_session`、HttpOnly、SameSite=Lax、Path=/、Max-Age=2592000、不設 Secure）／`cleared_cookie`（Max-Age=0）；常數 `SESSION_COOKIE_NAME`、`SESSION_DAYS = 30`。移除 noop 敘述與 `noop_auth`。
  - `backend/src/lib.rs`：移除 `auth::noop_auth` 的 `.layer(...)` 與 `use axum::middleware;`（見偏離說明）；`pub mod auth;` 保留，middleware 掛載留待票 02。
- 執行過的指令與結果（全綠）：
  - `cargo test --manifest-path backend/Cargo.toml --lib config` → 12 passed（含 auth 測試因名稱 substring 一併被篩入）。
  - `cargo test --manifest-path backend/Cargo.toml --lib auth` → 21 passed、0 failed。
  - `cargo fmt --manifest-path backend/Cargo.toml -- --check` → 通過（先以 `cargo fmt` 套用格式）。
  - `pnpm test`（`cargo test` 全量）→ 全部 test binary ok、0 failed，exit 0。
  - `cargo clippy --all-targets` → 僅既有檔案（assignments／import／ips 等）既有警告，本票新增／修改檔案無警告。
- 偏離說明：
  - Cargo.toml 額外加入 `time = "0.3"`：`axum-extra`／`cookie` 的 `Cookie::set_max_age` 需要 `time::Duration`，而 `axum-extra` 未 re-export `time`；此為傳遞依賴（已隨 cookie 0.18.2 進 lock，time 0.3.55），非新增網路下載。
  - 票面「移除 `noop_auth`」與「不接線」並存：因 `lib.rs` 原引用 `auth::noop_auth`，保留會編譯失敗；故移除該 layer（原為完全放行，行為不變）與對應 import，票 02 再改掛 `auth::require_login`。AppState／API／middleware 均未動。

