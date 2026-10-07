# 02: 前端——「新增網段」呈現與表單提示

**What to build:** `api/kea.ts` 型別（`KeaPlanSubnet.subnet_add?`、`KeaSyncPlan.totals.subnet_add`、`KeaApplySubnet.subnet_added`／`subnet_add_error?`）；`KeaSyncDialog.vue`（`canApply` 納入、計畫 banner／列／明細「新增網段」、`hasDetail()`、結果 `subnet_added`／`subnet_add_error` 與通知）；`SubnetFormDialog.vue` hint＋範圍 rule；`SubnetImportDialog.vue` 文案；`KeaStatusPage.vue` 不一致提示鬆綁。詳見 `.scratch/kea-subnet-create/spec.md`。

**Blocked by:** 01（型別契約已定於 spec；端到端驗證待後端）

**Status:** ready-for-agent

- [ ] `api/kea.ts` 型別
- [ ] `KeaSyncDialog`：按鈕條件、計畫／結果呈現、失敗以 negative＋部分失敗通知
- [ ] `SubnetFormDialog`：hint 與 `0 < id < 4294967295` rule
- [ ] `SubnetImportDialog` 文案；`KeaStatusPage` 提示鬆綁
- [ ] `pnpm lint:check`／`pnpm --filter frontend typecheck` 綠；票檔 Comments＋commit（不 push）
