# 04 — 前端：資產借還頁、路由與側欄

Status: done
Blocked by: 02

## 目標

實作 `spec §6`：「資產借還」頁面（出借中區塊＋快速歸還、已歸還紀錄分頁），並掛路由與側欄入口。

## 範圍

- 新增 `frontend/src/pages/LendingsPage.vue`：
  - 上方區塊「出借中」：`GET /lendings?returned=false`，不分頁全列。
    - 欄位：資產（property_no＋描述）、借用人、借出時間、預計歸還日、操作。
    - 「快速歸還」：`q-btn` 一鍵 `returnLending(id)`；不彈確認框；呼叫期間 loading 防連點；成功 `$q.notify({ type: "positive", message: "已歸還" })` 後重取上下兩區塊。
    - 逾期：`overdue === true` → 預計歸還日旁 `q-chip` 紅色標籤「逾期」。
    - 空狀態：「目前沒有出借中的資產」。
  - 下方區塊「已歸還紀錄」：伺服器端分頁 `q-table`（仿 `AssetsPage.vue` 的 `pagination`＋`@request` 模式；預設每頁 10、`rowsNumber` 取 `total`；無排序選項、無搜尋）。
    - 欄位：資產、借用人、借出時間、歸還時間、預計歸還日。
  - 時間顯示：`toLocaleString("zh-TW", { hour12: false })`（比照 `KeaLeasesPage.vue`）；日期原樣顯示；null 欄位顯示「—」。
- `frontend/src/router/routes.ts`：`children` 新增 `{ path: "lendings", component: () => import("@/pages/LendingsPage.vue") }`。
- `frontend/src/layouts/MainLayout.vue`：側欄「資產管理」區段、資產清單項目之下新增「資產借還」入口（`q-item clickable :to`，active 判定比照既有 `route.path.startsWith` 模式）。

## 驗收

- `pnpm --filter frontend typecheck`、`pnpm lint:check`、`pnpm build:frontend`。
- 手動檢核（`pnpm dev`）：
  - 側欄出現「資產借還」；路由 `/lendings` 直開正常。
  - 出借中資產出現在上方區塊（資產、借用人、時間、預計歸還日）；逾期者顯示紅色標籤。
  - 點「快速歸還」→ 該筆移出上方區塊、下方已歸還紀錄出現（含歸還時間），上下區塊同步更新。
  - 已歸還紀錄分頁：切頁、筆數正確、依借出時間倒序。
  - 無出借中資產時顯示空狀態。

## 注意

- 區塊標題與側欄文字一律「資產借還」／「出借中」／「已歸還紀錄」。
- 不要 `git commit`；不要動 `.scratch/` 內其他票。

## Comments

- 2026-10-07（票 04 完成）：新增 `frontend/src/pages/LendingsPage.vue`（頁面標題「資產借還」；上方「出借中」`listOpenLendings()` 不分頁全列——`rowsPerPage: 0`＋`hide-bottom`，欄位：資產（`assetLabel`＝property_no＋描述）、借用人、借出時間、預計歸還日、操作；「快速歸還」`q-btn` 一鍵 `returnLending(id)`、不彈確認框、以 `returningId` loading 防連點、成功 notify「已歸還」後 `Promise.all` 重取上下兩區塊、失敗 notify negative；`overdue` 於預計歸還日旁顯示紅色 `q-chip`「逾期」；空狀態「目前沒有出借中的資產」；下方「已歸還紀錄」伺服器端分頁 `q-table`——`v-model:pagination`＋`@request`、預設每頁 10、`rowsNumber` 取 `total`、無排序無搜尋，欄位：資產、借用人、借出時間、歸還時間、預計歸還日；時間 `toLocaleString("zh-TW", { hour12: false })`、日期原樣、null 顯示「—」）。`frontend/src/router/routes.ts` children 新增 `{ path: "lendings", … }`；`frontend/src/layouts/MainLayout.vue` 側欄「資產管理」項目之下新增「資產借還」（`q-item clickable :to`，active 以 `route.path.startsWith("/lendings")` 判定）。
  - 驗收：`pnpm --filter frontend typecheck`、`pnpm lint:check`（oxfmt＋oxlint）、`pnpm --filter frontend build`（SFC 編譯）全部通過。
  - 偏離／補充說明：(1) 資產欄以票 03 的 `assetLabel`（`編號(描述)`）呈現 property_no＋描述，沿用既有工具函式；(2) 「快速歸還」的 `:disable` 為 `returningId !== null && returningId !== props.row.id`——處理中的那筆保持可用，讓該按鈕的 loading 轉圈正常顯示，其餘按鈕停用；函式內再以 `returningId` 檢查防重入；(3) 歸還成功後僅重取兩區塊（維持目前頁碼），不主動跳回第 1 頁——規格未要求，且當筆記錄依 `lent_at` 倒序多半落在已歸還首頁。
