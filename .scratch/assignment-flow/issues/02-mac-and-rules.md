# 02 — mac.ts 與共用純規則

Status: done
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

- 實作完成，commit：`8582d316838b7a3c744e582abc35c8412aabc59c`（`指派流程：mac 與共用純規則（票 02）`）；本票檔變更不隨 commit。
- 驗收結果（全綠）：
  - `pnpm --filter frontend test`：4 test files、31 tests passed（本票新增 25：`mac.test.ts` 11、`assignmentFlow.test.ts` 14；餘為票 01 的 errors 3、text 3）。
  - `pnpm --filter frontend typecheck`：`vue-tsc --noEmit` 無錯（exit 0）。
  - `pnpm lint:check`：oxfmt --check 82 檔格式正確、oxlint 無錯（exit 0）。
- 新增檔：
  - `frontend/src/utils/mac.ts`：`normalizeMac`／`macRule`。
  - `frontend/src/utils/assignmentFlow.ts`（純規則區）：`interfaceLabel`／`assignmentPurposeLabel`／`purposeOptionsFor`／`interfaceNameRule`／`statusBlockMessage`／`cancelAssignmentConfirmMessage`；不 import 真 API（僅 `import type` 型別＋`utils/cidr.parseAddress`）。
  - `frontend/src/utils/mac.test.ts`、`frontend/src/utils/assignmentFlow.test.ts`（與被測檔同目錄）。
- 替換清單（只換住處；本地函式刪除，呼叫端與文案不變）：
  - `components/AssignIpDialog.vue`：刪本地 `normalizeMac`／`macRule`／`newInterfaceNameRule`／`interfaceLabel`／`assignmentPurposeLabel`／`purposeOptions` computed；模板名稱規則改 `:rules="[interfaceNameRule(newInterface.mac)]"`；`queryStatusHint` 三個負面文案改用 `statusBlockMessage(status.status)`（`static`／`reservation` 分支與顏色照舊）；`confirmCancelAssignment` 改用 `cancelAssignmentConfirmMessage(item.address)`。
  - `components/AssignmentDialog.vue`：同上五項（`interfaceLabel`／`purposeOptions`／`macRule`／`normalizeMac`／名稱規則）；`confirmCancel` 改用 `cancelAssignmentConfirmMessage(address.value)`，`isV6` 不再用於該訊息。
  - `components/AssetFormDialog.vue`：`normalizeMac`／`macRule`／`interfaceNameRule(row)`／`assignmentPurposeLabel` 改 import；模板改 `:rules="[interfaceNameRule(row.mac)]"`；`confirmCancelAssignment` 改用 `cancelAssignmentConfirmMessage`；移除僅供該訊息使用的 `parseAddress` 與已無用的 `IpPurpose` import；流程函式（`syncInterfaces`／`landAssignmentDrafts`／`preflightAssignmentDrafts`／`assignFromAssetWithTransfer`…）原地不動。
  - `pages/IpListPage.vue`：刪本地 `interfaceLabel`（第 4 份收斂）→ 改 import。
- 逐字核對（腳本比對舊碼）：三份 `normalizeMac`、三份 `macRule`、三份 `interfaceLabel`、兩份 `assignmentPurposeLabel` 的函式本體與新 util **完全一致**；兩份 `purposeOptions` 物件字面、三則阻擋訊息、六處取消確認文案（v4／v6）、名稱必填訊息亦逐字相同；無不一致。
- 取捨：
  - `queryStatusHint` 重構為「先 `statusBlockMessage`（非 `null` ＝負面）再 switch `static`／`reservation`」；`available` 與其他狀態仍回 `null`，訊息與顏色等價。
  - `interfaceNameRule` 由「關閉 `row`／`newInterface` ref」改為「以 `mac` 字串參數建立閉包」，模板傳入當下 `mac`，驗證時機與結果不變。
  - 曾以 `pnpm --filter frontend lint` 修正 4 檔排版（`oxfmt`），僅格式、無語意變更。
  - `utils/assignmentFlow.ts` 目前僅含純規則；流程層（FlowDeps／deps adapter／module 測試）留待票 03。
