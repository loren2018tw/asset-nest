# 06 — 整合驗證

Status: done
Blocked by: 01, 02, 03, 04, 05

## 目標

對本 spec 全部票的實作做最終驗證，確認規格落地且無回歸。

## 步驟

1. 讀 `.scratch/login-auth/spec.md` 與 `GLOSSARY.md`「認證詞彙」、`docs/adr/0021`，逐條核對。
2. 執行：
   - `cargo fmt --manifest-path backend/Cargo.toml -- --check`
   - `pnpm test`
   - `pnpm --filter frontend typecheck`
   - `pnpm lint:check`
   - `pnpm build:frontend`
   - `bash -n deploy/install.sh`
3. `git status --short`、`git diff --stat` 檢視範圍：不得動 `agent/`；`deploy/` 僅依票 05；確認 `.scratch/login-auth/`、`GLOSSARY.md`、`docs/adr/0021*` 在。
4. 端到端（建置後單機或 `pnpm dev`）：
   - 未登入：`/api/v1/assets` → 401；`/api/health` → 200；代理端點回自身語意（503／401）。
   - 登入 → cookie 屬性正確（HttpOnly／Lax／Max-Age／無 Secure）；`/api/v1/session` → username；登出 → 204；再存取 → 401。
   - 重啟後端 → 既有 cookie 仍有效（無狀態）；改 `AUTH_PASSWORD` 重啟 → 舊 cookie 立即 401。
   - 預設值情境：不設 `AUTH_*` 啟動 → `admin/admin` 可登入且有啟動警告 log。
   - 前端：深連結導向、登入後回原頁、過期通知、登出流程、右上角 `Loren`／GitHub（新視窗）／`V0.1.0`。
5. 規格落差與問題回報（在票 01–05 範圍內可直接修，否則回報）。

## 產出

- 驗證報告（回覆）：每項指令與檢核結果、發現的問題與修正。
- 失敗項目：列出檔案、行號、輸出與建議。

## 注意

- 不要 `git commit`；不要主動改規格或 GLOSSARY（除錯字）。

## Comments

- 2026-10-07 整合驗證完成（驗證 subagent）：全數通過、無需修正；瀏覽器 UI 項目 5 類列人工待驗。

### 1. 規格逐條核對（spec／GLOSSARY「認證詞彙」／ADR-0021）

- §3.1 設定：`Config.auth_username/auth_password`、`parse_auth_field`（trim；空／缺 → `admin`）、`DEFAULT_AUTH_*` 與單元測試齊全 → 符合。
- §3.2 簽章核心：claims `v1|{expiry}|{username}`（`splitn(3)`，username 可含 `|`）、金鑰 `SHA-256(prefix‖0x00‖u‖0x00‖p)`、HMAC-SHA256、常數時間比對（`verify_slice`／`subtle`）、30 天固定、cookie 屬性與清除 cookie → 符合（單元測試 21 案）。
- §3.3 AppState／middleware：`auth: Option<AuthConfig>`、`new()` 不附掛、`with_auth`；`main.rs` 一律附掛＋預設值警告；白名單 method＋path 完全相符；`None` 全放行 → 符合。
- §4 API：`/session`、`/login`（缺欄位／壞 JSON 400；錯帳密固定訊息 401；成功 Set-Cookie）、`/logout`（204＋清除、冪等）→ 符合。
- §5 前端：`api/auth.ts`、client 401 掛鉤（排除 `/login`）、auth store、`App.vue` 攔截、路由守衛、LoginPage（含 redirect 白名單）、MainLayout（Loren／GitHub inline SVG／`V{version}`／登出）、`frontend/package.json` 0.1.0 → 程式碼核對符合；互動項目見人工待驗。
- §6 安裝／文件／CI：`configure_auth_credentials`（重跑沿用、輸出不含密碼值）、`write_env_file`、`print_summary`、`.env.example`、README、`install-test.yml` 先登入再查 → 符合。
- GLOSSARY 三詞條與 ADR-0021 各決策（明文、不提供停用、無狀態 cookie、白名單、無 Secure）皆與實作一致。

### 2. 指令結果（全數 exit 0）

| 指令 | 結果 |
|---|---|
| `cargo fmt --manifest-path backend/Cargo.toml -- --check` | 通過 |
| `pnpm test` | 通過（28 binaries／390 tests passed；`tests/auth.rs` 12 案） |
| `pnpm --filter frontend typecheck` | 通過 |
| `pnpm lint:check` | 通過（oxfmt＋oxlint） |
| `pnpm build:frontend` | 通過（`frontend/dist/spa` 產出） |
| `bash -n deploy/install.sh` | 通過 |

### 3. 範圍檢視（git status／diff）

- `agent/` 無任何變更；`deploy/` 僅 `install.sh` 之票 05 變更（`uninstall.sh` 未動）。
- `.scratch/login-auth/`、`GLOSSARY.md`（認證詞彙）、`docs/adr/0021-single-account-login.md` 均在。
- 使用者 WIP（`frontend/src/components/*`、`pages/AssetsPage.vue`、`pages/IpListPage.vue`、`composables/`、`utils/debounce.ts`、`.scratch/ime-composition/`、`.env`、`asset-nest.db*`）未觸碰。
- 範圍外既有未提交變更（非本票、驗證過程未動）：`docs/adr/0019` 修訂、`docs/adr/0022`、`.scratch/install-nginx/`——屬 install-nginx spec 規劃產物（其票皆 `ready-for-agent`，實作尚未落地，故 `install.sh` 目前不含 nginx 變更）。

### 4. 端到端（獨立環境：`BIND_ADDR=127.0.0.1:18080`、`DATABASE_URL=sqlite:///tmp/opencode/login-auth-verify.db`、`WEB_DIST_DIR=frontend/dist/spa`；驗畢已 kill）

預設情境（不設 `AUTH_*`）：

- 啟動 log 有「登入帳密仍為預設值（admin），建議設定 AUTH_USERNAME／AUTH_PASSWORD」警告。
- `/api/health` → 200（`version:"0.1.0"` 即 `Cargo.toml` 真實來源）；未登入 `GET /api/v1/assets`、`GET /api/v1/session` → 401「請先登入」。
- `POST /api/v1/login` admin/admin → 200＋`Set-Cookie: asset_nest_session=…; HttpOnly; SameSite=Lax; Path=/; Max-Age=2592000`（無 Secure）。
- 帶 cookie `GET /api/v1/session` → 200 `{"username":"admin"}`。
- `POST /api/v1/logout` → 204＋`asset_nest_session=; …; Max-Age=0`；再次登出 204（冪等）；清除後以 jar 再存取 → 401。
- 未登入 `POST /api/v1/agents/heartbeat`（無 X-Auth-Code、未設 `AGENT_AUTH_CODE`）→ 503「後端未設定代理認證碼…」，非登入 401。
- 錯帳密 → 401「帳號或密碼錯誤」；缺欄位／壞 JSON → 400；登入後未知 `/api/v1/nope` → 404；`GET /api/v1/login`（方法非白名單）→ 401。
- 靜態檔免登入：`/`、`/kea/status`、`/ips`、`/assets/` → 200（`/assets` 因與建置產物目錄同名先 307 再 200；既有 ServeDir 行為、與登入無關）。

重啟與帳密變更：

- 同帳密重啟 → 既有 cookie 仍 200（無狀態、免重登）。
- `AUTH_PASSWORD=changed-pass` 重啟 → 舊 cookie 401；admin/admin 401；admin/changed-pass 200＋新 cookie 可用。

停用選項檢查：後端僅讀 `AUTH_USERNAME`／`AUTH_PASSWORD`，repo 內無任何 disable／enable／skip 認證的環境變數或開關；`main.rs` 一律 `with_auth` → 確認不存在停用登入的設定。

### 5. 發現與修正

- 無需修正項：票 01–05 未發現需修改之缺陷，驗證過程未更動任何實作檔案。
- 記錄性觀察（設計事實／範圍外，非缺陷）：
  1. 登出為無狀態（清除 cookie）；手動重放登出前的原始 token 至到期前仍有效——與 ADR-0021「登出＝清除 cookie」及已知 cookie 冒用風險記載一致。本票步驟「再用舊 cookie → 401」以瀏覽器語意（cookie 已清除）驗證通過。
  2. `RUST_LOG=debug` 時 `tracing::debug!(?config)` 會輸出 `auth_password`（既有 pattern 亦輸出 KEA／代理機密）；部署預設 filter 為 `info`，不會輸出。
  3. `apiDownload()` 不經 `send()`，下載端點 401 不觸發全域攔截（票面僅要求 `send()`；非缺陷）。

### 6. 人工待驗（此環境無法測瀏覽器 UI）

- 票 03：未登入直開 `/assets` → `/login?redirect=…`、登入成功回原頁；已登入直開 `/login` → `/kea/status`；錯帳密顯示「帳號或密碼錯誤」不導向；工作階段失效操作顯示「登入已過期，請重新登入」並轉登入頁；`redirect=https://evil.example`／`//evil`／`/a\b` 一律回首頁。
- 票 04：右上角 `Loren`＋GitHub 圖示（新視窗、`rel=noopener`；連結與屬性已經程式碼核對）＋`V0.1.0`（後端真實來源已驗證）＋登出鈕；登出通知「已登出」轉登入頁；失敗通知「登出失敗，請重試」。
- 票 05：CI `install-test` 手動觸發（待 install-nginx 票 04 合併後）。

以上皆需瀏覽器／CI 環境，於本機驗證未執行、不得視為已通過。
