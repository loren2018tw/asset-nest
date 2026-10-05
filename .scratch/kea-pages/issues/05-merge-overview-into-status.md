# 05: 總覽整併入系統狀態（移除總覽頁）

**What to build:** 原「總覽」頁（後端健康檢查）整併入「系統狀態」頁：新增「asset-nest 服務」區塊（狀態／服務／版本／資料庫），與 Kea 狀態並行載入、錯誤各自呈現；移除 `IndexPage.vue` 與側邊欄「總覽」項目；`/` 導向 `/kea/status`。

**Blocked by:** None (can start immediately)

**Status:** done

- [x] 系統狀態頁新增「asset-nest 服務」區塊；`/api/health` 與 `/api/v1/kea/status` 並行載入，失敗各自呈現（後端連線失敗顯示 banner，不影響 Kea 區塊）
- [x] 移除 `frontend/src/pages/IndexPage.vue`、側邊欄「總覽」項目；`routes.ts` 的 `/` 改為 `redirect: "/kea/status"`
- [x] README 與 kea-pages spec 更新；typecheck／lint／build 全綠

## Comments

實作完成（2026-10-05）。

- `KeaStatusPage.vue`：`load()` 以 `Promise.allSettled` 並行呼叫 `getKeaStatus()` 與 `fetchHealth()`；Kea 失敗照舊顯示頁頂 banner，健康檢查失敗於區塊內顯示「無法連線後端：…」；`updatedAt` 於每次重新整理完成後更新。「重新整理」按鈕同時更新兩者。
- `MainLayout.vue`：移除「總覽」項目（含 `dashboard` icon）；`routes.ts`：`/` 由 `IndexPage` 改為 `redirect: "/kea/status"`；`ErrorNotFound.vue` 的 Go Home 按鈕沿用 `/`（自動導向系統狀態）。
- 取捨：整合後頁面維持在「Kea」區段（最小變動）；`/` 導向 `/kea/status` 作為預設落地頁。
- 驗收：`pnpm --filter frontend typecheck`、`pnpm lint:check`、`pnpm build:frontend` 全綠（建置清單已無 `IndexPage` chunk）。
