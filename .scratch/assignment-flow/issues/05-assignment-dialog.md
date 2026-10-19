# 05 — AssignmentDialog 接線

Status: done
Blocked by: 03

## 目標

把 `AssignmentDialog.vue`（IP 清單頁的指派／編輯／v6 登錄）改接 module 與 `NewInterfaceForm`，刪除本地重複。

## 範圍

- `AssignmentDialog.vue` 改接：
  - `submit` v4：呼叫 `assignFromSubnetFlow`；成功「已更新指派／已指派」＋`emit("saved")`＋關窗；錯誤進 banner。
  - `submit` v6：未編輯時呼叫 `registerIpv6Flow`（成功「已登錄並指派」＋`emit`＋關窗；v6 **不通知 Kea**）；編輯時 Enter 不動作（照舊）。
  - `cancel`：`cancelAssignmentFlow`；成功「已取消指派」＋`emit`＋關窗；失敗進 banner（與 AssignIp 的 toast 不同，照舊）。
  - `confirmCancel` 文案改用 `cancelAssignmentConfirmMessage`。
  - 「新增介面」區塊改用 `NewInterfaceForm`（`assetId`＝所選資產 id、`disabled`＝未選資產；@created 後加入清單、選取）。
  - 刪除本地：`createNewInterface`／`submit`／`submitRegistry`／`cancel` 的流程本體與規則重複（票 02 已換者不重複）。
  - 留下：資產搜尋、`selectedMac`、`dialogTitle`／`submitLabel`、`prepare`、`ipv6Rule` 的 UI 驗證等表單呈現。
- 不動：`AssignIpDialog.vue`（票 04）、`AssetFormDialog.vue`、`assignTransfer.ts`。

## 驗收

- `pnpm --filter frontend test`、`pnpm --filter frontend typecheck`、`pnpm lint:check` 綠。
- `pnpm build:frontend` 綠。
- 行為核對：三條流程的驗證訊息、通知（v6 無 Kea）、emit、關窗與現況逐字相同。

## 交付

1. commit：`指派流程：AssignmentDialog 接線（票 05）`。
2. 本票檔：`Status: done`，`## Comments` 補記（commit hash、刪除的行數、取捨）。

## Comments

- commit：`c4e7f81`（`指派流程：AssignmentDialog 接線（票 05）`，僅含本票三檔；`.scratch` 未 commit）；`05d045f`（`指派流程：票 05 補強——換資產僅收合新增介面表單`，含 `NewInterfaceForm.vue`＋`AssignmentDialog.vue`，見下）。
- 行數：`AssignmentDialog.vue` 644 → 515 行（淨 −129；diff +57／−186）；`assignmentFlow.ts` +17／−13（`isIpv6` 移入共用純規則區並 export，原處刪除）；`assignmentFlow.test.ts` +17（`isIpv6` 三案例：`fd00::10` 含去空白為真、`::ffff:1.2.3.4` 為真、`1.2.3.4` 與 `fd00::gg` 為假）。
- 指令結果：`pnpm --filter frontend test` 5 檔 101 tests 全綠（98 → 101）；`pnpm --filter frontend typecheck` 綠；`pnpm lint:check` 綠（oxfmt／oxlint）；`pnpm build:frontend` 綠。補強 `05d045f` 後四指令重跑，結果同上。
- 刪除：`createNewInterface`、`submitRegistry` 的流程本體（保留同名薄殼改呼叫 `registerIpv6Flow`，比照票 04 `submitDraft` 風格）、`submit`／`cancel` 流程本體、`newInterface`／`newInterfaceError`／`showNewInterface`／`savingInterface` 狀態、`fillNewInterfaceMac`、本地 `isIpv6`；import 收斂：`createInterface`／`assignIp`／`registerIp`／`cancelAssignment`／`notifyKeaSync`／`textOrNull`／`macRule`／`normalizeMac`／`interfaceNameRule`／`PeerMacHint`。
- 取捨（新增介面表單的重設與收合，補強 `05d045f`）：`prepare()`（每次開啟）沿用票 04 補強（65ed81e）的 `:key="newInterfaceFormKey"` bump＝重設語意（清欄位、清錯誤、收合，等價舊碼開啟時對 `showNewInterface`／`newInterface`／`newInterfaceError` 的重設）。`onAssetSelected()`（更換資產）改呼叫子元件 expose 的 `collapse()`（`NewInterfaceForm.vue` 新增 `defineExpose`），僅收合、**欄位值與錯誤訊息照舊保留**，逐字等價舊碼的 `showNewInterface = false`；該處的 key bump 已移除。`:key` 未見於票面 markup 草案（由實作補上）。
- 行為核對：v4 驗證順序「保留 MAC→編輯／選介面」、成功 positive「已更新指派／已指派」＋`emit("saved")`＋關窗、warnings 逐筆＋Kea 提示照舊；v6 編輯模式 Enter 不動作，未編輯走 `registerIpv6Flow`（warnings 逐筆、不通知 Kea、成功「已登錄並指派」）；`cancel` 成功「已取消指派」，失敗 `{ok:false}` 進 banner（與 AssignIp 的 toast 不同，照舊），Kea 提示先於 positive toast；`confirmCancel` 的 `cancelAssignmentConfirmMessage`＋`cancelAssignmentHint` 串接、`html`／`cancel`／`persistent` 照舊（票 02 已換）。
- 備註：`saving`／`cancelling` 的 loading 現在包住整個流程呼叫（含同步驗證），與票 04 AssignIpDialog 的接線一致；驗證錯誤時 loading 僅一瞬。`messageOf` 仍留於對話框（資產搜尋／讀取介面錯誤訊息）。
