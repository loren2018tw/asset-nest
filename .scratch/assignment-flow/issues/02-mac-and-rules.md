# 02 — mac.ts 與共用純規則

Status: todo
Blocked by: 01

## 目標

依 spec §介面（共用純規則）：建立 `utils/mac.ts` 與 `utils/assignmentFlow.ts` 的純規則區，並把三個對話框／IpListPage 的逐字重複規則層替換掉。**本票不搬任何流程函式**（submit／createNewInterface／syncInterfaces／land… 原地不動）。

## 範圍

- 新增 `frontend/src/utils/mac.ts`：`normalizeMac(text): string | null`、`macRule(value: string | null): true | string`（徒手輸入格式與錯誤訊息逐字沿用）。
- 新增 `frontend/src/utils/assignmentFlow.ts`（本票只含純規則與型別）：
  - `interfaceLabel`、`assignmentPurposeLabel`、`purposeOptionsFor`、`interfaceNameRule(mac: string)`、`statusBlockMessage(status: IpQueryStatusKind)`、`cancelAssignmentConfirmMessage(address: string)`（文案逐字）。
  - 繁中 JSDoc；不 import 任何真 API。
- 新增測試：
  - `utils/mac.test.ts`（見 spec §測試決策）。
  - `utils/assignmentFlow.test.ts`（本票只測純規則）。
- 接線替換（行為逐字等價、只換住處）：
  - `AssignIpDialog.vue`：`normalizeMac`／`macRule`／`newInterfaceNameRule`→`interfaceNameRule(newInterface.mac)`／`interfaceLabel`／`purposeOptions`→`purposeOptionsFor(selectedMac)`／`assignmentPurposeLabel`；
  - `AssignmentDialog.vue`：同上（其 `interfaceLabel`／`purposeOptions`／`newInterfaceNameRule`）；
  - `AssetFormDialog.vue`：`normalizeMac`／`macRule`／`interfaceNameRule(row)`→`interfaceNameRule(row.mac)`／`assignmentPurposeLabel`；
  - `IpListPage.vue`：`interfaceLabel` 改 import（第 4 份收斂）；
  - 三處取消確認文案：`AssignIpDialog.confirmCancelAssignment`、`AssignmentDialog.confirmCancel`、`AssetFormDialog.confirmCancelAssignment` 的訊息改用 `cancelAssignmentConfirmMessage(address)`；
  - `AssignIpDialog.queryStatusHint` 的三個負面文案改用 `statusBlockMessage(status.status)`；`static`／`reservation` 分支與顏色照舊。
- 不動：`createNewInterface`、`submit`／`submitDraft`、`syncInterfaces`、`landAssignmentDrafts`、`preflightAssignmentDrafts`、`assignFromAssetWithTransfer`、`assignTransfer.ts`、`assignmentDraft.ts`。

## 驗收

- `pnpm --filter frontend test` 綠。
- `pnpm --filter frontend typecheck`、`pnpm lint:check` 綠。
- 手動核對：三對話框與 IpListPage 的規則訊息與現況逐字相同。

## 交付

1. commit：`指派流程：mac 與共用純規則（票 02）`。
2. 本票檔：`Status: done`，`## Comments` 補記（commit hash、測試數、替換清單）。

## Comments
