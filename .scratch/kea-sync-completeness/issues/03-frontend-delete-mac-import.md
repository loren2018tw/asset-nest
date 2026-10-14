# 03: 前端：刪除／MAC 推送與匯入提示

**What to build:** 前端承接新的 `kea_sync` 回應並警示失敗；匯入完成頁條件提示完整同步（見 `.scratch/kea-sync-completeness/spec.md`、`docs/adr/0027`）。

**Blocked by:** 01、02

**Status:** ready-for-agent

- [ ] `api/assets.ts`：`AssetDeleted { kea_sync }`、`deleteAsset` 回傳之；`ImportCreated.managed_reservations`
- [ ] `api/interfaces.ts`：`InterfaceSaved.kea_sync?`、`InterfaceDeleted`；`deleteInterface` 回傳之
- [ ] `AssetFormDialog.vue`：`syncInterfaces`（介面刪除／MAC 編輯）與 `deleteCreatedAsset` 失敗警示
- [ ] `AssetsPage.vue`：刪除後失敗警示
- [ ] `AssetImportDialog.vue`：步驟③ `managed_reservations > 0` 提示「請執行 Kea 完整同步」
- [ ] `pnpm lint:check`、typecheck、build 全綠

## Comments
