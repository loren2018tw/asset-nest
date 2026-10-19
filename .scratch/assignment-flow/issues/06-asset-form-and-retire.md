# 06 — AssetFormDialog 接線、影響摘要與退場

Status: done
Blocked by: 04, 05

## 目標

把 `AssetFormDialog.vue` 的儲存落地管線改接 module、資產影響摘要收斂到 `utils/assetImpact.ts`、`utils/assignTransfer.ts` 退場，並做全卡驗收與 spec 補記。

## 範圍

- 新增 `frontend/src/utils/assetImpact.ts`：`summarizeAssetImpact`（見 spec §介面）＋`assetImpact.test.ts`。
- `frontend/src/pages/AssetsPage.vue`：`confirmDeleteWithImpact` 的計數改用 `summarizeAssetImpact`（訊息組裝與 `escapeHtml` 照舊）。
- `frontend/src/components/AssetFormDialog.vue`：
  - `submit` 保留三步序列：`saveAssetBody` →（`emit("saved")`）→ `syncInterfaces` →（warnings 已由 module 提示）→ `landAssignmentDrafts`；`emit("saved")`／`refreshAssignments`／「草稿清空才關窗」與現況相同。
  - `savedAssetId` 由 `saveAssetBody` 的 `createdId` 寫回。
  - `preflightAssignmentDrafts` 改 import module 純函式。
  - `disposalImpact` 改用 `summarizeAssetImpact`（`originalInterfaces`／`assignments`／`lastSeenAt`）。
  - 本地 `InterfaceDraft`／`AssignmentDraft` 型別改 `extends` module 的 `InterfaceRow`／`AssignmentDraftRow`（`key` 等欄位留在對話框）。
  - 刪除本地已無用途的流程本體與 helper（`syncInterfaces`／`landAssignmentDrafts`／`preflightAssignmentDrafts`／`interfaceChanged`／`toInterfaceInput` 等）。
- 退場：`assignTransfer.ts` 刪檔——`confirmTransfer`（含 `textOf`／`nullableTextOf` 使用者只在該檔）逐字移入 `assignmentFlowDeps.ts`；確認全 repo 無 `assignTransfer` 引用。
- 全卡驗收（獨立重跑）：`pnpm --filter frontend test`、`pnpm --filter frontend typecheck`、`pnpm lint:check`、`pnpm build:frontend`；grep 核對 module／共用 util 之外無本地重複（`normalizeMac`／`messageOf`／`textOrNull`／`interfaceLabel`／`assignmentPurposeLabel`／`cancelAssignmentConfirmMessage`／`createNewInterface`／`preflightAssignmentDrafts`）。
- spec 補記實作記錄（含實作 commits 與驗收數字）。

## 驗收

- 上述指令全綠。
- 行為核對：AssetFormDialog 的三步序列、emit 時機、關窗條件、失敗保留狀態與現況逐字相同。

## 交付

1. commit：`指派流程：AssetFormDialog 接線與 assignTransfer 退場（票 06）`。
2. commit：`指派流程：spec 補記實作記錄`。
3. 本票檔：`Status: done`，`## Comments` 補記（commit hash、grep 結果、取捨）。

## Comments

- commit：`6dce9d2`（`指派流程：AssetFormDialog 接線與 assignTransfer 退場（票 06）`；含本票 8 檔；`.scratch` 未 commit；未 push）。票面交付的第 2 個 commit（spec 補記）改由上游統一處理，本票未動 `spec.md`。
- 行數：`AssetFormDialog.vue` 1459 → 1300 行（淨 −159；diff +63／−222）；新增 `assetImpact.ts` 39 行、`assetImpact.test.ts` 79 行（6 案例）；`AssetsPage.vue` +9／−15；`assignmentFlowDeps.ts` +32／−4（`confirmTransfer` 逐字移入，`textOf`／`nullableTextOf` 只在已移入 module 的 `parseAssignedElsewhere` 內使用，未隨行）；`assignmentFlow.ts` +1／−1（`AssignedElsewhere` 註解去 `assignTransfer` 字樣）；`NewInterfaceForm.vue` +2／−2（見取捨 1）；刪 `assignTransfer.ts` −103 行。合計 8 檔 +225／−347。
- 指令結果（獨立重跑，全綠）：`pnpm --filter frontend test` → 6 檔 107 tests 過（101 → 107；＋`assetImpact.test.ts`）；`pnpm --filter frontend typecheck` → 過；`pnpm lint:check`（oxfmt／oxlint）→ 過；`pnpm build:frontend` → 過。
- grep 核對：
  - `grep -rn "function normalizeMac\|function messageOf\|function textOrNull\|function interfaceLabel\|function assignmentPurposeLabel\|function createNewInterface\|function preflightAssignmentDrafts\|assignFromAssetWithTransfer" frontend/src` →
    `frontend/src/utils/mac.ts:7:export function normalizeMac(text: string): string | null {`、
    `frontend/src/utils/assignmentFlow.ts:112:export function interfaceLabel(`、
    `frontend/src/utils/assignmentFlow.ts:121:export function assignmentPurposeLabel(purpose: IpPurpose): string {`、
    `frontend/src/utils/assignmentFlow.ts:211:export function preflightAssignmentDrafts(`、
    `frontend/src/utils/text.ts:5:export function textOrNull(value: string): string | null {`、
    `frontend/src/utils/errors.ts:5:export function messageOf(cause: unknown): string {`
    （僅共用 util／module，無對話框本地定義；`assignFromAssetWithTransfer` 已無任何命中）。
  - `grep -rn "assignTransfer" frontend/src` → 0 筆（exit 1）。
- 行為核對：`submit` 三步序列、emit 時機、關窗條件、失敗保留逐字沿用——`preflight`（改呼叫 module 純函式，訊息不變）→ `saveAssetBody`（`createdId` 寫回 `savedAssetId`）→ 立即 `emit("saved")` → `syncInterfaces`（kea 取第一筆 failed 提示一次、warnings 逐筆，皆由 module 在同步結束後提示，順序與現況相同）→ 草稿非空才 `landAssignmentDrafts`（逐筆 warnings＋kea；錯誤或移轉取消→`land.error` 進 banner、`land.drafts` 保留草稿）→「emit＋`refreshAssignments`」→ 草稿清空才關窗。`AssetFormDialog` 與 `AssetsPage` 的影響摘要皆改走 `summarizeAssetImpact`，`lastSeen`／`macLabel` 輸出與原兩份實作逐字相同（測試釘住）；`cancelAssignmentItem` 改走 `cancelAssignmentFlow`（成功「已取消指派」＋emit＋重載；失敗 negative toast，Kea 提示在流程內、時機照舊），`cancellingAssignmentId` loading 照舊。
- 取捨與差異：
  1. **`NewInterfaceForm.vue` 存量清理（票外、為驗收 grep）**：驗收要求 `function createNewInterface` 不得殘留於對話框，票 04 交付的 `NewInterfaceForm.vue` 按鈕處理器仍沿用舊流程名；僅改名 `submitNewInterface`（含模板 `@click`），行為零變動（本體自票 04 起即為 `createInterfaceFlow` 薄殼）。
  2. `InterfaceDraft`／`AssignmentDraft` 用別名（`type X = Y`）而非 `extends`（票面兩者皆可）；JSDoc 語意保留（`id === null`＝未儲存、`rowKey` 綁列）。`AssignmentDraftInput` import 仍因 props／`onDraftAssigned` 留用。
  3. `submit` 的 `interfaceDrafts.value = sync.rows` 為同一陣列回寫（module 就地改列）；`originalInterfaces.value = sync.original` 於刪除後接新陣列，照 module 契約。
  4. 草稿為空時不再呼叫 `landAssignmentDrafts`（現況為呼叫後立即早退，無 emit／refresh）；對外行為相同，且少一次無謂呼叫。
  5. `assignmentFlow.ts` 的 `AssignedElsewhere` 與 `assignmentFlowDeps.ts` 的 adapter 註解改寫，移除 `assignTransfer` 字樣以滿足「frontend/src 零引用」的 grep 驗收（歷史出處留在 `.scratch` 文件）。
  6. 保留未動：`toInput`、`refreshAssignments`、`draftAssignContext`／`existingAssignmentsFor`／`onDraftAssigned`（草稿顯示狀態）、`requestCancel` 家族、`confirmDispose`（Kea 提示照舊）、`deleteCreatedAsset`（`notifyKeaSync` 仍為對話框用途上的直接使用處，連同 `confirmDispose`）。
  7. 未做（依上游指示）：`spec.md` 補記、push；票檔本體（Status／Comments）不入 commit。
