# 04: 前端：受管網段警示

**What to build:** 刪除受管網段與編輯受管網段（CIDR 變更、`kea_subnet_id` 清空／改號）時的 Kea 殘留警示（見 `.scratch/kea-sync-completeness/spec.md`、`docs/adr/0027`）。

**Blocked by:** None

**Status:** done

- [x] `SubnetsPage.vue`：刪除確認框——受管網段加註「Kea 端網段與位址池不會被移除，需自行清理」（以詳情讀取 `kea_subnet_id`）
- [x] `SubnetFormDialog.vue`：留存載入時的原 CIDR／`kea_subnet_id`；編輯受管網段且 CIDR 變更或 `kea_subnet_id` 清空／改號 → 存檔前 `$q.dialog` 二次確認
- [x] `pnpm lint:check`、typecheck、build 全綠

## Comments

實作完成（主實作 commit `f541296`）。

- `SubnetsPage.confirmDelete`：改為 async、先 `fetchSubnet` 讀詳情；受管網段於確認框以 `text-warning` 加註殘留說明（`html: true`；名稱經 `escapeHtml`）；讀取失敗不進入刪除流程。
- `SubnetFormDialog`：新增 `loadedOriginal`（`prepare` 時自詳情留存原 CIDR／`kea_subnet_id`）與 `managedChangeWarning`；僅「原為受管」且 CIDR 變更或 `kea_subnet_id` 清空／改號時顯示「變更受管網段」二次確認，確定後才 `save`（原 `submit` 拆分為 `submit`＋`save`）。
- 驗證：`pnpm lint:check`、typecheck、build 全綠。
