# 04 — NewInterfaceForm 與 AssignIpDialog 接線

Status: todo
Blocked by: 03

## 目標

建立共用元件 `components/NewInterfaceForm.vue`，並把 `AssignIpDialog.vue` 改接 module（live／草稿流程、取消流程、規則），刪除已無用途的本地重複。

## 範圍

- 新增 `frontend/src/components/NewInterfaceForm.vue`：
  - props `{ assetId: number | null; disabled?: boolean }`；emits `created(saved: InterfaceSaved)`。
  - 內容＝現行兩對話框逐字相同的「新增介面」區（按鈕＋卡片＋`PeerMacHint`＋名稱／MAC 欄與規則＋建立按鈕＋錯誤 banner）；內部狀態照舊；`createInterfaceFlow` 以 `assignmentFlowDeps($q)` 取得 deps。
  - 差異以 props 表達：`disabled`（AssignmentDialog 用「未選資產」）；AssignIpDialog 不傳（預設 false）。
- `AssignIpDialog.vue` 改接：
  - `submit`：live 分支呼叫 `assignFromAssetFlow`；`"saved"`→「已移轉並指派／已指派」＋`emit("saved")`＋關窗、`"cancelled"`→靜默、`"error"`→banner；草稿分支呼叫 `draftAssignmentFlow`（`statusLookup`＝本地 `statusForAddress` 快取）。
  - `createNewInterface` 改由 `NewInterfaceForm`（@created 後加入清單、選取、收合）。
  - `cancelAssignmentItem` 用 `cancelAssignmentFlow`（成功「已取消指派」＋`emit`＋重載；失敗 negative toast）；確認框文案用 `cancelAssignmentConfirmMessage`。
  - 刪除本地：`submitDraft`／`createNewInterface`／`sameSubnetConflict`（已移入 assignmentDraft）／`normalizeMac`／`newInterfaceNameRule`／`macRule` 等（票 02 已換者不重複）。
  - `statusForAddress` 快取與 `queryStatusHint` 留在對話框（顏色與 static／reservation 分支照舊）。
- 不動：`AssignmentDialog.vue`、`AssetFormDialog.vue`、`AssetsPage.vue`、`assignTransfer.ts`。

## 驗收

- `pnpm --filter frontend test`、`pnpm --filter frontend typecheck`、`pnpm lint:check` 綠。
- `pnpm build:frontend` 綠。
- 行為核對：AssignIpDialog 兩模式的通知、emit、關窗、錯誤呈現與現況逐字相同。

## 交付

1. commit：`指派流程：NewInterfaceForm 與 AssignIpDialog 接線（票 04）`。
2. 本票檔：`Status: done`，`## Comments` 補記（commit hash、刪除的行數、取捨）。

## Comments
