# 20: 已指派 IP 清單的取消指派入口（指派對話框、資產編輯對話框）

**What to build:** 「指派 IP」對話框若該資產已有指派，於表單下方列出其已指派 IP，每列提供「取消指派」；資產編輯對話框的已指派 IP 清單每列也提供「取消指派」。取消皆先確認、成功後通知並即時更新清單。

**Blocked by:** None (can start immediately)

**Status:** done

- [x] 指派對話框：載入該資產已指派 IP，欄位下方以清單顯示（無指派時不顯示）；每列「取消指派」
- [x] 資產編輯對話框：「已指派 IP」清單每列於「IP 管理」旁新增「取消指派」
- [x] 兩處皆先確認（v4 回「可用」、v6 自登錄清單移除）；成功後通知「已取消指派」並更新清單，失敗 notify 顯示
- [x] 取消成功後通知父層刷新（指派對話框 emit `saved`；資產編輯對話框更新自身清單並 emit `saved`）
- [x] 規格追加定案；typecheck／lint／build 全綠

## Comments

實作完成。

- 前端（未動後端；沿用既有 `DELETE /subnets/{id}/ips/{address}/assignment`）：
  - `frontend/src/components/AssignIpDialog.vue`：`prepare()` 由 `fetchAsset` 一併載入 `assignments`；表單下方新增「已指派 IP」區塊（無指派時不顯示），每列顯示位址、網段（名稱｜CIDR）、用途、介面（名稱／MAC）與 hostname，列尾「取消指派」按鈕（逐列 loading、其他列停用）。
  - `frontend/src/components/AssetFormDialog.vue`：「已指派 IP」清單每列於「IP 管理」旁新增「取消指派」按鈕；標題移除「（唯讀）」、說明改為「指派由『指派 IP』入口進行；IP 值不可修改。」；`onAssignSaved` 抽出 `refreshAssignments` 並補 emit `saved`（指派／取消後外層資產清單的「已指派 IP」欄即時刷新）。
  - 兩處取消皆先以 `$q.dialog` 確認（v4：「回到可用」；v6：「自登錄清單移除」，以 `parseAddress` 判斷地址族），成功後 notify「已取消指派」並重載清單；失敗 notify 錯誤訊息。
- 決策／取捨：
  - 指派對話框列出的「已指派 IP」為該資產全部指派（不限固定介面）：可同時看到其他介面的既有指派，避免重複。
  - 資產編輯對話框的「取消指派」僅即時更新清單與外層，不動未儲存的表單／介面草稿。
- 規格：`spec.md` 追加定案（2026-10-05，第二批）；§4.1、§4.3 同步。
- 驗收：`pnpm --filter frontend typecheck`、`pnpm --filter frontend lint:check`、`pnpm --filter frontend build` 全綠。
