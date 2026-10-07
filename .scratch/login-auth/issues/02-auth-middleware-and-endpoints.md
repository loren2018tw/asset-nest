# 02 — 後端：登入 middleware 與 API 端點

Status: done
Blocked by: 01

## 目標

實作 `spec §3.3、§4`：`AppState` 掛載、`require_login` middleware（免登入白名單）、`/api/v1/session|login|logout` 端點與整合測試。

## 範圍

- `backend/src/lib.rs`：
  - `AppState` 新增 `auth: Option<auth::AuthConfig>`；`new()` 維持不附掛（既有測試不動）；新增 builder `with_auth(username, password)`。
  - `noop_auth` 移除；`/api` 子樹改掛 `middleware::from_fn_with_state(state.clone(), auth::require_login)`。
  - `require_login`：`state.auth` 為 `None` → 放行；白名單（method＋path 完全相符）→ 放行；否則驗證 cookie 的 `asset_nest_session`，無效／逾期 → `ApiError::unauthorized("請先登入")`（401 JSON）。
  - 白名單：`GET /api/health`、`POST /api/v1/login`、`POST /api/v1/logout`、`POST /api/v1/agents/heartbeat`、`POST /api/v1/agents/observations`。
- `backend/src/main.rs`：`AppState` 一律 `with_auth(config.auth_username, config.auth_password)`；若任一欄位為預設值 `admin` → `tracing::warn!`（例如「登入帳密仍為預設值（admin），建議設定 AUTH_USERNAME／AUTH_PASSWORD」）。
- `backend/src/api/auth.rs` 新模組＋`api/mod.rs` merge：
  - `GET /session` → 200 `{ "username": <設定帳號> }`。
  - `POST /login`（Json body `{username, password}`；`Json` rejection → 400）→ `credentials_match` 失敗 401「帳號或密碼錯誤」；成功 200 `{ "username" }`＋`Set-Cookie`（`session_cookie`）。
  - `POST /logout` → 204＋`cleared_cookie`（免登入、冪等）。
- `backend/tests/auth.rs`（新；比照既有測試 helper：記憶體 SQLite＋migrations＋`oneshot`＋注入 `ConnectInfo`；state 附掛 `with_auth("admin", "secret-01")`）：
  - 登入成功：200、Set-Cookie 含 `asset_nest_session`、HttpOnly、SameSite=Lax、`Max-Age=2592000`、無 Secure。
  - 帳密錯 → 401「帳號或密碼錯誤」；缺欄位／壞 JSON → 400。
  - `/session` 未登入 401；登入後 200 且 username 正確。
  - logout → 204 且含清除 cookie；再次 logout 仍 204。
  - 未登入 `GET /api/v1/assets` → 401；帶 cookie → 200。
  - 未登入未知 `/api/v1/nope` → 401；登入後 → 404。
  - 白名單：未登入 `GET /api/health` → 200；未登入 `POST /api/v1/agents/heartbeat` → 既有代理語意（未設 `AGENT_AUTH_CODE` 回 503／不符回 401），**不得**回登入 401。
  - 過期 token（以過去的 now 簽）→ 401。
  - cookie 內容無效（亂字串）→ 401。
- 既有測試全數不動、維持全綠。

## 驗收

- `pnpm test` 全綠（含新 `tests/auth.rs`）；`cargo fmt -- --check`。

## 注意

- middleware 以 `from_fn_with_state` 取得 `AppState`；白名單比對在 middleware 內（注意 method 精確比對，`GET /api/v1/login` 應 401）。
- 401 訊息字串固定供前端依賴：「請先登入」「帳號或密碼錯誤」。
- 不要 `git commit`；不要動 `.scratch/` 內其他票。

## Comments

- 2026-10-07 完成（agent）。
  - 變更：
    - `lib.rs`：`AppState.auth: Option<auth::AuthConfig>`；`new()` 維持 `None`；新增 `with_auth(username, password)`；`/api` 子樹掛 `middleware::from_fn_with_state(state.clone(), auth::require_login)`。
    - `auth.rs`：新增 `require_login`（`None` 放行；白名單 method＋path 完全相符放行；其餘驗證 `asset_nest_session`，無效／逾期回 401「請先登入」）與 `is_public_endpoint`，附單元測試。
    - `main.rs`：一律 `with_auth(config.auth_username, config.auth_password)`；任一欄位為預設值時 `tracing::warn!`。
    - `api/auth.rs`（新）：`GET /session`、`POST /login`（`Json` rejection → 400；帳密錯 401「帳號或密碼錯誤」；成功 200＋`Set-Cookie`）、`POST /logout`（204＋清除 cookie）並 merge 進 `api/mod.rs`。
    - `tests/auth.rs`（新，12 案）：登入成功 cookie 屬性、帳密錯、缺欄位／壞 JSON、`/session`、logout 冪等、受保護端點、未知路徑（未登入 401／登入後 404）、白名單（health 200；代理端點 503／401 非登入 401）、逾期 token、無效 cookie、`GET /api/v1/login` → 401。
  - 驗收：`pnpm test` 全綠（exit 0；含 `tests/auth.rs` 12 案全過）；`cargo fmt --manifest-path backend/Cargo.toml -- --check` 通過。
  - 偏離：無。備註：`state.auth` 為 `None` 時 `/session` 回 401「請先登入」、`/login` 回 401「帳號或密碼錯誤」（未附掛即無可登入的成功路徑）；`AppState::new()` 維持不附掛認證，既有測試未動。
