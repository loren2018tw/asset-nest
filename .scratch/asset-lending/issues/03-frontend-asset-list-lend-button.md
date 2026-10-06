# 03 — 前端：資產清單借出按鈕與對話框

Status: done
Blocked by: 02

## 目標

實作 `spec §5`：資產清單操作欄的「借出」按鈕（含出借中停用態）與 `LendingDialog`。

## 範圍

- 新增 `frontend/src/api/lendings.ts`：`createLending(assetId, input)`、`returnLending(id)`、`listOpenLendings()`、`listReturnedLendings(query)`、`listBorrowers()`；型別 `Lending`、`LendingWithAsset`、`LendingInput`、`LendingBrief`（camelCase，對映後端 snake_case）。
- `frontend/src/api/assets.ts`：`AssetListRow` 新增 `lending?: LendingBrief | null`。
- `frontend/src/components/LendingDialog.vue`（比照 `AssignIpDialog.vue` 的 v-model＋`@saved` 模式）：
  - 標題區顯示該資產 property_no 與描述。
  - 借用人：`q-select use-input`，選項 `listBorrowers()`（自由輸入仍可送出）；必填、trim 非空即時提示。
  - 預計歸還日：`q-input type="date"`（選填）。
  - 備註：`q-input type="textarea"`（選填）。
  - 送出：`createLending` → `$q.notify({ type: "positive", message: "已借出" })` → 關閉並觸發 `saved`。
- `frontend/src/pages/AssetsPage.vue` 操作欄最左：
  - 未出借（`row.lending == null`）：`q-btn flat dense round icon="person_add" aria-label="借出"` → `openLend(row)` 開 dialog；成功後重取清單。
  - 出借中：`q-btn flat no-caps label="出借中" color="orange" disable`＋tooltip「出借中：{row.lending.borrower}」；該列刪除按鈕停用＋tooltip「出借中，請先歸還」；編輯按鈕維持可用。
  - 不加新欄位、不加 chip。
- 前端 API 錯誤呈現：409 與 400 以既有錯誤訊息模式顯示（`$q.notify` negative）。

## 驗收

- `pnpm --filter frontend typecheck`、`pnpm lint:check`。
- 手動檢核（`pnpm dev`）：
  - 未出借列：借出按鈕可點、dialog 欄位與資產資訊正確、送出後清單該列按鈕變「出借中」橘色停用。
  - 出借中列：hover 顯示借用人；刪除按鈕停用；編輯仍可開。
  - 借用人建議：既有借出人名出現在下拉、可自由輸入新名。

## 注意

- 按鈕排列：借出在最左，其後維持 指派 IP／編輯／刪除。
- 不要 `git commit`；不要動 `.scratch/` 內其他票。

## Comments

- 2026-10-07（票 03 完成）：新增 `frontend/src/api/lendings.ts`（`Lending`、`LendingWithAsset`、`LendingBrief`、`LendingInput` 型別與 `createLending`、`returnLending`、`listOpenLendings`、`listReturnedLendings`、`listBorrowers`）；`assets.ts` 的 `AssetListRow` 新增 `lending?: LendingBrief | null`；新增 `frontend/src/components/LendingDialog.vue`（借用人 `q-select use-input`＋既有借用人建議＋自由輸入、trim 非空即時提示；預計歸還日／備註選填；送出成功 notify「已借出」→ `saved`）；`AssetsPage.vue` 操作欄最左借出按鈕（未出借）／「出借中」橘色停用＋tooltip（出借中），刪除按鈕出借中停用＋tooltip，編輯維持可用，`saved` 後重取清單。
  - 驗收：`pnpm --filter frontend typecheck`、`pnpm lint:check`（oxfmt＋oxlint）、`pnpm --filter frontend build`（SFC 編譯）全部通過。
  - 偏離票文說明：(1) 票文「型別 camelCase，對映後端 snake_case」與全站慣例衝突——既有 API 模組（`assets.ts`、`ips.ts` 等）型別欄位直接採 snake_case 對映後端 JSON，無任何 camelCase 轉換層；票 02 已因同理由採 snake_case，本票比照辦理（`lent_at`、`due_at` 等）；(2) tooltip 掛在外層 `<span>` 而非停用按鈕內——停用按鈕不觸發滑鼠事件，q-tooltip 須以外層元素為錨點，否則 hover 不顯示借用人；(3) 票文未含的 `listReturnedLendings` 查詢參數以 `{ page?, per_page? }` 設計（`returned=true` 固定帶入），供票 04 使用。
