# 02: 前端——「新增網段」呈現與表單提示

**What to build:** `api/kea.ts` 型別（`KeaPlanSubnet.subnet_add?`、`KeaSyncPlan.totals.subnet_add`、`KeaApplySubnet.subnet_added`／`subnet_add_error?`）；`KeaSyncDialog.vue`（`canApply` 納入、計畫 banner／列／明細「新增網段」、`hasDetail()`、結果 `subnet_added`／`subnet_add_error` 與通知）；`SubnetFormDialog.vue` hint＋範圍 rule；`SubnetImportDialog.vue` 文案；`KeaStatusPage.vue` 不一致提示鬆綁。詳見 `.scratch/kea-subnet-create/spec.md`。

**Blocked by:** 01（型別契約已定於 spec；端到端驗證待後端）

**Status:** done

- [x] `api/kea.ts` 型別
- [x] `KeaSyncDialog`：按鈕條件、計畫／結果呈現、失敗以 negative＋部分失敗通知
- [x] `SubnetFormDialog`：hint 與 `0 < id < 4294967295` rule
- [x] `SubnetImportDialog` 文案；`KeaStatusPage` 提示鬆綁
- [x] `pnpm lint:check`／`pnpm --filter frontend typecheck` 綠；票檔 Comments＋commit（不 push）

## Comments

實作完成（主實作 commit `4bf3146`，`02 Kea 網段建立：前端「新增網段」呈現與表單提示`）。

- `frontend/src/api/kea.ts`：新增 `KeaSubnetAddPlan`（`pools: string[]`、`gateway: string | null`）；`KeaPlanSubnet` 增 `subnet_add?: KeaSubnetAddPlan | null`（沿用 `gateway?` 的選填風格）、`KeaSyncPlan.totals` 增 `subnet_add: number`、`KeaApplySubnet` 增 `subnet_added: boolean` 與 `subnet_add_error?: string | null`。
- `frontend/src/components/KeaSyncDialog.vue`：
  - `canApply` 納入 `totals.subnet_add`：僅有「將建立網段」時也能按「套用同步」。
  - 計畫 banner 加「、新增網段 N」；每網段列有 `subnet_add` 時加註「、將建立 Kea 網段」。
  - 計畫明細新增「新增網段」段落：標頭顯示 Kea id，`pools` 逐段列出（空池顯示「無位址池」），gateway 期望值以 `?? "（未設）"` 呈現；`hasDetail()` 納入 `subnet_add`。
  - 結果 banner 加「、已建立網段 N」（`subnet_added === true` 的網段數）；結果網段列有 `subnet_added` 時加註「、已建立 Kea 網段」。
  - `subnet_add_error` 以 negative 行顯示；`$q.notify` 部分失敗判斷納入 `subnet_add_error`。
- `frontend/src/components/SubnetFormDialog.vue`：hint 改「選填；僅 IPv4，全系統唯一；Kea 尚無此 subnet-id 時，完整同步會建立」；rule 擴為整數且 `0 < id < 4294967295`（超界訊息「Kea subnet-id 須介於 1 與 4294967294」，對齊後端）。
- `frontend/src/components/SubnetImportDialog.vue`：Kea subnet-id 說明補「為整數、介於 1 與 4294967294、全系統唯一，僅 IPv4；Kea 尚無此 subnet-id 時，可由完整同步建立」。
- `frontend/src/pages/KeaStatusPage.vue`：不一致 chip 的 tooltip 鬆綁為「本地受管網段數與 Kea 不一致；可能只是尚未完整同步（可由完整同步補建缺少的受管網段），也可能是 kea_subnet_id 設定待確認」。
- 驗收：`pnpm lint:check` 綠（oxfmt＋oxlint、65 檔）、`pnpm --filter frontend typecheck`（vue-tsc）綠；前端無測試基礎設施未新增測試（同 spec）。本票未動後端。
- 判斷與落差：
  - `subnet_add_error` 不加前端前綴：後端 `kea::sync` 已組「建立 Kea 網段失敗：{error}」，直接以 negative 顯示即為該文案（`settings_error` 後端無前綴，故維持前端加「網段層設定同步失敗：」）。
  - 明細空池顯示「無位址池」：呼應 ADR-0023「全空網段仍建立」，spec 未明列此情境。
  - 型別 `subnet_add` 用 `KeaSubnetAddPlan | null`（契約為 `subnet_add?`；後端僅在有值時序列化，`| null` 只為與 `gateway?` 風格一致）。
  - 端到端（真機同步補建）依票 03。
