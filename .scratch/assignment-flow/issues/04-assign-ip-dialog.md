# 04 — NewInterfaceForm 與 AssignIpDialog 接線

Status: done
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

- commit：`03844cd`（`指派流程：NewInterfaceForm 與 AssignIpDialog 接線（票 04）`；只含本票 2 檔，`.scratch` 未入版控）；補強 `65ed81e`（`指派流程：票 04 補強——開啟時重設新增介面表單`；只含 `AssignIpDialog.vue`，+5 行）。
- 指令結果：`pnpm --filter frontend test` 98/98 過（未新增測試，純元件接線）；`pnpm --filter frontend typecheck` 過；`pnpm lint:check` 過（oxfmt＋oxlint；新檔首跑曾被 oxfmt 調整 import 一行，已修）；`pnpm build:frontend` 過。補強 `65ed81e` 後四項重跑亦全綠。
- 行數：`AssignIpDialog.vue` 858→694（−164 行；diff −222／+58）；新增 `NewInterfaceForm.vue` 127 行（含逐字移入的「新增介面」區與 `createInterfaceFlow` 接線）。
- 取捨與差異：
  1. 子元件「新增介面」按鈕 `:disable="disabled || saving"`（原 `savingInterface`）、建立鈕 `:loading="saving"`；AssignIpDialog 不傳 `disabled`（預設 false），停用語意與現況相同。
  2. 驗證錯誤時 `saving` 的切換：現況本地驗證早退、通過後才設 `savingInterface`；改接後驗證住在流程內，`saving` 先設真、`{error}` 後於 `finally` 設回。流程驗證同步完成，Vue 於同一 microtask 批次更新，不會畫出 loading 閃爍；最終狀態與現況相同（emit／關窗／錯誤呈現不受影響）。
  3. live 模式的「無目標資產」防守：現況為驗證（格式→介面→保留 MAC）之後靜默返回；改接後此防守在流程呼叫前（`props.asset == null` 即返回），因此該情境不再顯示驗證訊息。兩個呼叫端在 live 模式皆保證 asset 非 null（AssetsPage `v-if="assignAsset !== null"`；AssetFormDialog 只由已落地介面列進入），差異不可達；草稿模式（asset 可為 null）順序不變＝草稿分支仍在防守之前。
  4. `prepare()` 原本重設 `showNewInterface`／`newInterface`／`newInterfaceError`，改接後這些狀態屬子元件的「內部狀態」；子元件在 QDialog 關閉（hide transition 結束）後卸載、每次開啟重新掛載即自然歸零，唯一縫隙是「關窗動畫約 300ms 內又開啟」會沿用同一實例（舊碼此刻會就地重設）。**補強（`65ed81e`）**：`AssignIpDialog` 加 `newInterfaceFormKey = ref(0)`，於 `prepare()`（watch `modelValue` immediate，只在開啟時呼叫，呼叫時機未動）內 `+= 1`，模板掛 `:key="newInterfaceFormKey"` 重建子元件——每次開啟都與舊碼重設等價，正常開關與關閉動畫不受影響。
  5. `onSaved` 只留正評 toast＋`emit("saved")`＋關窗；warnings 與 Kea 提示由流程負責（時機、順序、次數不變）。`cancelAssignmentItem` 保留 `catch`：`refreshAssignments()` 失敗仍以 negative toast 呈現（現況語意），流程錯誤則走 `{ok:false}` 的 negative toast。
  6. 移除的 import／本地重複：`PeerMacHint`（移入子元件）、`createInterface`／`cancelAssignment`、`assignFromAssetWithTransfer`、`notifyKeaSync`、`textOrNull`、`macRule`、`interfaceNameRule`、`sameSubnetConflict`／`resolveSubnetByAddress`／`subnetDisplayLabel`（本地使用消失，`assignmentDraft` 只留 `AssignmentDraftInput` 型別）。仍留：`parseAddress`（`addressRule`／`isV6`）、`normalizeMac`（`selectedMac`／`fixedInterfaceLabel`）、`statusForAddress` 快取、`queryStatusHint`。
  7. 模板掛載照票面：`<new-interface-form :asset-id="props.asset?.id ?? null" @created="onInterfaceCreated" />` 置於 `isFixed` 的 `v-else` 內、介面 `q-select` 之後（補強後另帶 `:key="newInterfaceFormKey"`）；`@created` 內加入清單並選取。
