# 06 — AssetFormDialog 接線、影響摘要與退場

Status: todo
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
