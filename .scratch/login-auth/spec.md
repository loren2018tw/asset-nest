# spec — 登入與工作階段（login-auth）

- 狀態：已定案（經 2026-10-07 grilling 兩輪逐題確認，全部採納建議）
- 詞彙：`GLOSSARY.md`「認證詞彙」（工作階段、登入、登出）已登錄
- 決策：`docs/adr/0021-single-account-login.md`（已接受）
- 相關：nginx 前門與 XFF 信任模型見 `.scratch/install-nginx/spec.md`（ADR-0022）；兩者同動 `deploy/install.sh`、`install-test.yml`，建議本 spec 先落地

## 1. 背景與目標

系統目前無登入驗證（`backend/src/auth.rs` 為 no-op 插槽；`lib.rs` 掛載 `noop_auth`）。目標：

1. 單一帳號登入，帳密由環境設定（`AUTH_USERNAME`／`AUTH_PASSWORD`）；未設定或空白時逐欄位套用預設 `admin`／`admin`。
2. 未登入使用前端一律導向登入頁；登入後右上角有登出按鈕。
3. 工作階段自登入起 1 個月（30 天）過期，不續期。
4. 版面右上角原「Quasar v…」改顯示 `Loren`＋GitHub 圖示（新視窗連專案首頁）＋專案版本號 `Vx.x.x`；版本以後端 `Cargo.toml`（`CARGO_PKG_VERSION`）為唯一真實來源，經 `GET /api/health` 取得。

## 2. 名詞與不變量

- **工作階段（session）**：登入後取得的存取狀態；固定 30 天後過期，登出或帳密變更即失效。不提供停用登入的設定。
- **免登入端點**：維持公開的端點白名單（§3.3）。
- 不變量：
  - `AUTH_USERNAME`／`AUTH_PASSWORD` 未設定或（trim 後）空白，各自套用預設值 `admin`；只要任一欄位仍為預設值，啟動時記警告。
  - 帳密任一變更 → 所有既有工作階段立即失效（簽章金鑰由帳密推導）。
  - 登入錯誤一律同一回應：`401 {error:"unauthorized", message:"帳號或密碼錯誤"}`，不區分帳號或密碼錯。
  - 密碼以明文存於 env（檔案權限 0640、root）；比對採常數時間。

## 3. 後端：設定、簽章與邊界

### 3.1 環境設定（`backend/src/config.rs`）

- `Config` 新增 `auth_username: String`、`auth_password: String`。
- 讀取 `AUTH_USERNAME`／`AUTH_PASSWORD`：`trim` 後為空或未設定 → `admin`（逐欄位）；非空原樣（trim 後）。無長度或字元限制。
- 常數 `DEFAULT_AUTH_USERNAME`／`DEFAULT_AUTH_PASSWORD`＝`"admin"`。
- `.env.example` 新增 `AUTH_*` 說明區塊（見 §6）。

### 3.2 簽章工作階段（`backend/src/auth.rs` 改寫）

- **Cookie**：`asset_nest_session`；`HttpOnly`、`SameSite=Lax`、`Path=/`、`Max-Age=2592000`（30 天）；**不設 `Secure`**（TLS 由部署層提供，見 ADR-0022）。
- **Token**：`base64url(claims).base64url(hmac)`；`claims = "v1|{expiry_unix}|{username}"`（驗證時 `splitn(3, '|')`，username 允許含 `|`）。
- **金鑰**：`SHA-256("asset-nest-session-v1" ‖ 0x00 ‖ username ‖ 0x00 ‖ password)`（改帳密即全數失效）。
- **簽發**：`expiry = now + 30 天`（UTC unix 秒）；簽章 HMAC-SHA256。
- **驗證**：簽章常數時間比對（`Mac::verify_slice` 或等效）；`now < expiry`；claims 的 username 與現行設定相符。任一不符 → 視同未登入。
- **期限**：固定 30 天常數，不隨活動延長；無環境變數可調。
- **帳密比對**：`credentials_match` 常數時間比對（`subtle` 或等效），帳號與密碼皆比。
- 新增依賴：`hmac`、`sha2`、`base64`、`subtle`、`axum-extra`（`cookie` feature；版本以 axum 0.8 相容為準）。
- 型別與函式：`AuthConfig { username, password }`、`sign_token(&AuthConfig, now) -> String`、`verify_token(&AuthConfig, &str, now) -> Option<String>`（`now` 可注入供測試）、`credentials_match`、`session_cookie(token)`／`cleared_cookie()`。

### 3.3 AppState 與 middleware

- `AppState` 新增 `auth: Option<auth::AuthConfig>`；`AppState::new()` 維持不附掛（既有測試不動），新增 `with_auth(username, password)` builder。**正式啟動（`main.rs`）一律附掛**（值來自 `Config`，未設定即 `admin/admin`）；不提供環境變數停用登入。
- `lib.rs`：`noop_auth` 置換為 `auth::require_login`（`middleware::from_fn_with_state`），掛載於 `/api` 子樹。
- **免登入白名單**（method＋path 完全相符；其餘一律 401）：

  | Method | Path |
  |---|---|
  | GET | `/api/health` |
  | POST | `/api/v1/login` |
  | POST | `/api/v1/logout` |
  | POST | `/api/v1/agents/heartbeat` |
  | POST | `/api/v1/agents/observations` |

- 未登入存取受保護端點（含未匹配的 `/api/*`）→ `401 {error:"unauthorized", message:"請先登入"}`；登入後未匹配 → 既有 JSON 404。
- 前端靜態檔（SPA）不經認證（原始碼公開；真正邊界在 API）。
- `state.auth` 為 `None`（測試路徑）→ 全數放行。

## 4. API

新模組 `backend/src/api/auth.rs`，掛載於 `api/mod.rs` 的 v1 路由。

| 方法 | 路徑 | 行為 |
|---|---|---|
| GET | `/api/v1/session` | 已登入 → 200 `{ "username": "…" }`；未登入由 middleware 回 401 |
| POST | `/api/v1/login` | body `{ username, password }`（缺欄位／非 JSON → 400）。比對失敗 → 401「帳號或密碼錯誤」；成功 → 200 `{ "username": … }`＋`Set-Cookie` |
| POST | `/api/v1/logout` | 一律 204＋清除 cookie（免登入、冪等） |

- 401 沿用 `ApiError::unauthorized`；登入失敗訊息固定為「帳號或密碼錯誤」。

## 5. 前端

### 5.1 API 與狀態

- `frontend/src/api/auth.ts`：`login(username, password): Promise<{username}>`、`logout(): Promise<void>`、`fetchSession(): Promise<{username}>`。
- `frontend/src/stores/auth.ts`（Pinia）：`username: string | null`、`checked: boolean`；`ensureSession()`（僅查一次；401 → 未登入）、`login()`（成功設 `username`、`checked`）、`logout()`（API 失敗 rethrow、成功清狀態）、`expire()`（清 `username`，供 401 攔截）。
- `frontend/src/api/client.ts`：新增 `setUnauthorizedHandler(handler)` 掛鉤；`send()` 收到 401 且路徑非 `/api/v1/login` 時呼叫（原始 `ApiError` 仍照常拋出）。
- **401 攔截行為**（掛鉤註冊處，如 `App.vue`）：`username` 非 null（原為已登入）→ 清狀態＋`$q.notify`「登入已過期，請重新登入」＋導向 `/login?redirect=<當前 fullPath>`；未登入（守衛查詢）→ 不通知（守衛自行導向）。

### 5.2 路由守衛與登入頁

- `router/index.ts` 新增 `beforeEach`：
  - 目標 `/login`：已登入 → 轉 `/kea/status`；未登入 → 放行。
  - 其他路由：`await auth.ensureSession()`；未登入 → `{ path: "/login", query: { redirect: to.fullPath } }`。
- `routes.ts` 新增 `/login`（MainLayout 之外、catch-all 之前）：`frontend/src/pages/LoginPage.vue`。
  - 卡片置中（無頁首／側欄）；標題「IT 資產整合管理系統」與「登入」。
  - 帳號欄（`autocomplete="username"`）＋密碼欄（`type="password"`、`autocomplete="current-password"`）；Enter 送出；送出中 loading 停用。
  - 錯誤顯示「帳號或密碼錯誤」，不區分欄位。
  - 成功 → `router.replace(redirect ?? "/kea/status")`；`redirect` 僅接受以 `/` 起頭、非 `//`、不含 `\` 的站內路徑，否則回首頁（防開放轉址）。
  - 不顯示 Loren／GitHub／版本（未登入不揭示資訊）。

### 5.3 版面（`MainLayout.vue`）

- 移除 `<div>Quasar v{{ $q.version }}</div>`；右上角改為：
  `Loren`（純文字）→ GitHub 圖示（新視窗連 `https://github.com/loren2018tw/asset-nest`，`target="_blank"`＋`rel="noopener"`，aria-label「GitHub 專案首頁」，**inline SVG**、`fill="currentColor"`）→ `V{version}`（小字）→ `q-separator vertical` → 登出鈕（`icon="logout"`，aria-label／tooltip「登出」）。
- 版本：登入後以既有 `fetchHealth()`（`frontend/src/api/health.ts`）取得；失敗不顯示版本文字。
- 登出：點擊直接執行（無確認框）；成功 → `$q.notify`「已登出」＋轉 `/login`；失敗 → 通知「登出失敗，請重試」、維持登入狀態。
- `frontend/package.json` 版本對齊 `0.1.0`（賣相一致；真實來源仍是 `backend/Cargo.toml`）。

## 6. 安裝、文件與 CI

- `deploy/install.sh`：
  - 新增 `configure_auth_credentials()`（比照 `configure_agent_auth_code`）：讀既有 `ENV_FILE` 的 `AUTH_USERNAME`／`AUTH_PASSWORD`，重跑一律沿用；缺漏時帳號 `admin`、密碼以 `generate_password` 隨機產生（首裝），info 顯示但**不印出密碼值**。
  - `write_env_file()` 寫入兩行（附註解指向 ADR-0021）。
  - `print_summary()`：加「登入帳密：grep '^AUTH_' ${ENV_FILE}」；移除「（本系統尚無登入驗證）」字樣（埠／防火牆措辭由 install-nginx spec 接手）。
- `.env.example`：新增 §3.1 說明（預設 admin/admin、空白即預設、變更即全體登出、30 天、明文與檔案權限註記）。
- `README.md`：一鍵安裝段補登入說明（預設帳密、變更方式、cookie 30 天、TLS 靠反向代理）。
- `.github/workflows/install-test.yml`：凡打受保護 `/api/v1/*`（如 `/api/v1/agents`）的步驟，先以 env 檔帳密 `POST /api/v1/login` 取 cookie 再查詢；`/api/health` 維持免登入直接查。（install-nginx 票 04 再改走 nginx 埠。）

## 7. 非目標（v1）

- 暴力破解防護／rate limit（部署以網路邊界防護）。
- 多帳號、角色權限、密碼變更 UI、密碼雜湊存放（env 明文是決策，見 ADR-0021）。
- 停用登入的設定。
- 工作階段天數可調、記住我、工作階段列表／個別撤銷。
- 跨來源部署（cookie 依賴同源；`VITE_API_BASE_URL` 跨來源的 CORS／credentials 不在範圍）。

## 8. 測試矩陣

| 範圍 | 重點 |
|---|---|
| config 單元 | 未設定／空白 → 預設 admin；trim；非空原樣 |
| auth 單元 | 簽章往返；過期（注入 now）；竄改 claims／簽章；改帳密即失效；claims username 不符；username 含 `\|`；帳密常數時間比對 |
| 整合（新 `backend/tests/auth.rs`） | 登入成功（Set-Cookie 屬性：HttpOnly／SameSite=Lax／Max-Age）；帳密錯 401；缺欄位 400；`/session` 未登入 401／已登入 200；logout 204 冪等；未登入存取受保護端點 401（assets 為代表）；白名單端點不受影響（health 200；代理端點回其既有語意，不回登入 401）；登入後未匹配 API 404 |
| 既有測試 | 全綠（`AppState::new()` 不附掛認證） |
| frontend | `pnpm --filter frontend typecheck`、`pnpm lint:check`、`pnpm build:frontend`；人工檢核見各票 |

## 9. 實作票

| # | 票 | 依賴 |
|---|----|------|
| 01 | 後端：認證設定與 session 簽章核心 | — |
| 02 | 後端：登入 middleware 與 API 端點 | 01 |
| 03 | 前端：登入頁與路由守衛（含 401 攔截） | 02 |
| 04 | 前端：登出按鈕與右上角資訊（Loren／GitHub／版本） | 03 |
| 05 | 安裝與文件：AUTH_*、摘要、.env.example、README、CI 登入修正 | 02 |
| 06 | 整合驗證 | 01–05 |

注意：票 05 與 install-nginx 票 02 同動 `deploy/install.sh`、與 install-nginx 票 04 同動 `install-test.yml`；建議本 spec 全數落地後再開 install-nginx。
