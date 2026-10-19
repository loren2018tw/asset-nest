# 03 — assignmentFlow 流程層與真 adapter

Status: todo
Blocked by: 02

## 目標

依 spec §介面（草稿／資產端／網段端）：補齊 `utils/assignmentFlow.ts` 的流程函式、型別與埠；新增真 adapter `utils/assignmentFlowDeps.ts`；`sameSubnetConflict` 自 AssignIpDialog 移入 `assignmentDraft.ts`。**本票不動任何對話框與面板**（`assignTransfer.ts` 也先留著；退場在票 06）。

## 範圍

- `frontend/src/utils/assignmentFlow.ts` 追加：
  - 型別：`AssignedElsewhere`、`FlowNotify`、`TransferConfirm`、`FlowApi`（以 `import type` ＋ `typeof` 取得簽章）、`FlowDeps`、`InterfaceRow`、`AssignmentDraftRow`。
  - `parseAssignedElsewhere`（自 `assignTransfer.ts` 逐字移入，含 `textOf`／`nullableTextOf`）。
  - 流程：`createInterfaceFlow`、`assignFromAssetFlow`、`draftAssignmentFlow`、`landAssignmentDrafts`、`saveAssetBody`、`syncInterfaces`、`assignFromSubnetFlow`、`registerIpv6Flow`、`cancelAssignmentFlow`。
  - 行為逐字沿用：驗證訊息、驗證順序、逐筆順序、遇錯即停、殘留草稿靜默清理、移轉取消語意、通知（warnings／kea）時機與次數（見 spec §行為等價表與 §逐字保留的分歧）。
  - 錯誤策略：`createInterfaceFlow`／`assignFromAssetFlow`／`landAssignmentDrafts`／網段端三流程把 API 錯誤轉結果型別；`syncInterfaces`／`saveAssetBody` 讓 API 錯誤向外拋（現行語意）。
- `frontend/src/utils/assignmentDraft.ts`：`sameSubnetConflict` 移入（自 AssignIpDialog 逐字）；新增 `assignmentDraft.test.ts`（`sameAddress`／`resolveSubnetByAddress`／`sameSubnetConflict`）。
- 新增 `frontend/src/utils/assignmentFlowDeps.ts`：
  - `assignmentFlowDeps($q: QVueGlobals): FlowDeps`；`api` 綁 `@/api/*` 真函式；`notify.warning`＝`$q.notify({type:"warning", message, timeout:6000})`、`notify.kea`＝`notifyKeaSync($q, sync)`；`confirm` 暫由 `assignTransfer.confirmTransfer` 轉接（票 06 移入）。
- `frontend/src/utils/assignmentFlow.test.ts` 追加流程測試（fake deps：記呼叫序、可注入錯誤）——清單見 spec §測試決策。
- 不動：三個對話框、`AssetFormDialog`、`AssetsPage`、`IpListPage`、`NewInterfaceForm`（尚未存在）、`assignTransfer.ts` 本體。

## 驗收

- `pnpm --filter frontend test` 綠（新增流程測試全數）。
- `pnpm --filter frontend typecheck`、`pnpm lint:check` 綠。
- module 無 `@/api/*` runtime import（以 grep 核對）。

## 交付

1. commit：`指派流程：流程層與真 adapter（票 03）`。
2. 本票檔：`Status: done`，`## Comments` 補記（commit hash、測試數、與 spec 的差異）。

## Comments
