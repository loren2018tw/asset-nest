# 04: 前端：受管網段警示

**What to build:** 刪除受管網段與編輯受管網段（CIDR 變更、`kea_subnet_id` 清空／改號）時的 Kea 殘留警示（見 `.scratch/kea-sync-completeness/spec.md`、`docs/adr/0027`）。

**Blocked by:** None

**Status:** ready-for-agent

- [ ] `SubnetsPage.vue`：刪除確認框——受管網段加註「Kea 端網段與位址池不會被移除，需自行清理」（以詳情讀取 `kea_subnet_id`）
- [ ] `SubnetFormDialog.vue`：留存載入時的原 CIDR／`kea_subnet_id`；編輯受管網段且 CIDR 變更或 `kea_subnet_id` 清空／改號 → 存檔前 `$q.dialog` 二次確認
- [ ] `pnpm lint:check`、typecheck、build 全綠

## Comments
