# 03 — 前端：登入頁與路由守衛（含 401 攔截）

Status: done
Blocked by: 02

## 目標

實作 `spec §5.1、§5.2`：登入頁、auth store、路由守衛與全域 401 攔截。本票不含 MainLayout 的登出按鈕與右上角資訊（票 04）。

## 範圍

- `frontend/src/api/auth.ts`（新）：
  - `login(username, password): Promise<{ username: string }>`（`POST /api/v1/login`）
  - `logout(): Promise<void>`（`POST /api/v1/logout`，204）
  - `fetchSession(): Promise<{ username: string }>`（`GET /api/v1/session`）
- `frontend/src/api/client.ts`：
  - 新增 `setUnauthorizedHandler(handler: (() => void) | null)`；`send()` 在 `response.status === 401` 且路徑非 `/api/v1/login` 時呼叫處理器（原 `ApiError` 仍照常拋出）。
- `frontend/src/stores/auth.ts`（新，Pinia）：
  - state：`username: string | null`、`checked: boolean`。
  - `ensureSession()`：`checked` → 直接回；否則 `fetchSession()`；401 → `username = null`；成功設 `username`；設 `checked = true`。
  - `login(u, p)`：成功設 `username`、`checked = true`。
  - `logout()`：呼叫 API；失敗 rethrow；成功清狀態（`username = null`）。
  - `expire()`：`username = null`（`checked` 維持 true，避免守衛再次探測）。
- 401 掛鉤註冊（`App.vue` 或等效初始化處）：`setUnauthorizedHandler` 內——
  - `username` 非 null（原為已登入）：`expire()`＋`$q.notify`「登入已過期，請重新登入」＋`router.push({ path: "/login", query: { redirect: router.currentRoute.value.fullPath } })`。
  - `username` 為 null：不通知、不導向（守衛自行處理）。
- `frontend/src/router/index.ts`：`beforeEach`——目標 `/login` 且已登入 → `/kea/status`；其他路由 `await ensureSession()`，未登入 → `{ path: "/login", query: { redirect: to.fullPath } }`。
- `frontend/src/router/routes.ts`：新增 `{ path: "/login", component: () => import("@/pages/LoginPage.vue") }`（MainLayout 之外、catch-all 之前）。
- `frontend/src/pages/LoginPage.vue`（新）：
  - 獨立版面（無頁首／側欄），卡片置中；標題「IT 資產整合管理系統」與「登入」。
  - 帳號 `q-input`（`autocomplete="username"`）；密碼 `q-input type="password"`（`autocomplete="current-password"`）；Enter 送出；送出中 loading／停用。
  - 錯誤顯示「帳號或密碼錯誤」（沿用後端 message）。
  - 成功 → `router.replace(redirect ?? "/kea/status")`；`redirect` 僅接受以 `/` 起頭、非 `//`、不含 `\` 的站內路徑，否則回首頁。

## 驗收

- `pnpm --filter frontend typecheck`、`pnpm lint:check`、`pnpm build:frontend`。
- 人工檢核（`pnpm dev`，後端需設定一組非預設帳密）：
  1. 未登入直開 `/assets` → 轉 `/login?redirect=%2Fassets`；登入成功回 `/assets`。
  2. 已登入直開 `/login` → 轉 `/kea/status`。
  3. 錯帳密顯示「帳號或密碼錯誤」、不導向、不觸發過期通知。
  4. 登入後刪除 cookie 再操作任一頁（或縮短 token 時效模擬）→ 顯示「登入已過期，請重新登入」並轉登入頁。
  5. `redirect=https://evil.example`、`redirect=//evil`、`redirect=/a\b` → 一律回首頁。

## 注意

- 不新增依賴（Pinia 已存在）；不改 `MainLayout`（票 04）。
- 不要 `git commit`；不要動 `.scratch/` 內其他票。

## Comments

- 2026-10-07（agent）：完成票 03。

### 變更摘要

- 新增 `frontend/src/api/auth.ts`：`login(u, p)`／`logout()`／`fetchSession()`（`POST /api/v1/login`、`POST /api/v1/logout`、`GET /api/v1/session`）。
- `frontend/src/api/client.ts`：新增 `setUnauthorizedHandler(handler)`；`send()` 於 `status === 401` 且路徑非 `/api/v1/login` 時呼叫處理器，`ApiError` 仍照常拋出。
- 新增 `frontend/src/stores/auth.ts`（Pinia）：`username`／`checked`；`ensureSession()`（僅查一次，失敗視同未登入）、`login()`（成功設帳號與 `checked`）、`logout()`（失敗 rethrow、成功清帳號）、`expire()`（清帳號、`checked` 不變）。
- `frontend/src/App.vue`：註冊 401 攔截——`username` 非 null → `expire()`＋`$q.notify`「登入已過期，請重新登入」＋帶當前 `fullPath` 轉 `/login`；`username` 為 null → 不動作（守衛自行導向）。
- `frontend/src/router/index.ts`：`beforeEach` 守衛——`/login` 已登入 → `/kea/status`；其他路由 `await ensureSession()`，未登入 → `/login?redirect=<fullPath>`。
- `frontend/src/router/routes.ts`：新增 `/login` 路由（MainLayout 之外、catch-all 之前）。
- 新增 `frontend/src/pages/LoginPage.vue`：獨立置中卡片（標題「IT 資產整合管理系統」＋「登入」）、帳號／密碼欄位（`autocomplete` 依 spec）、Enter 送出、送出中 loading 並停用、錯誤顯示後端訊息；成功 `router.replace(redirect ?? "/kea/status")`，`redirect` 白名單（`/` 開頭、非 `//`、不含 `\`）。

### 執行過的指令（皆通過）

- `pnpm --filter frontend typecheck` → 通過（無輸出）。
- `pnpm lint:check` → 通過（oxfmt 格式＋oxlint）。
- `pnpm build:frontend` → Build succeeded；產物含 `assets/LoginPage-*.js`，`frontend/dist/` 為 gitignore。

### 待人工項目（移票 06 人工／整合驗證）

以上指令通過後，票內「人工檢核」1–5 逐條待票 06 於瀏覽器＋實際後端（非預設帳密）驗證：

1. 未登入直開 `/assets` → 轉 `/login?redirect=%2Fassets`；登入成功回 `/assets`。
2. 已登入直開 `/login` → 轉 `/kea/status`。
3. 錯帳密顯示「帳號或密碼錯誤」、不導向、不觸發過期通知。
4. 登入後刪 cookie 再操作任一頁 → 顯示「登入已過期，請重新登入」並轉登入頁。
5. `redirect=https://evil.example`、`//evil`、`/a\b` → 一律回首頁。

### 偏離說明

- 守衛於 `/login` 且 `checked === false` 時亦先 `ensureSession()`，確保「已登入直開 `/login`」（含重新載入、cookie 仍有效）也能轉 `/kea/status`；票面僅寫「已登入 → 轉」，此為等價且更完整之實作。
- `logout()` API 以空 JSON body（`{}`）呼叫；後端忽略 body、固定回 204。
