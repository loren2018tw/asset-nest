# 05 — AssignmentDialog 接線

Status: todo
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
