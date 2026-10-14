# 03: 前端：刪除／MAC 推送與匯入提示

**What to build:** 前端承接新的 `kea_sync` 回應並警示失敗；匯入完成頁條件提示完整同步（見 `.scratch/kea-sync-completeness/spec.md`、`docs/adr/0027`）。

**Blocked by:** 01、02

**Status:** done

- [x] `api/assets.ts`：`AssetDeleted { kea_sync }`、`deleteAsset` 回傳之；`ImportCreated.managed_reservations`
- [x] `api/interfaces.ts`：`InterfaceSaved.kea_sync?`、`InterfaceDeleted`；`deleteInterface` 回傳之
- [x] `AssetFormDialog.vue`：`syncInterfaces`（介面刪除／MAC 編輯）與 `deleteCreatedAsset` 失敗警示
- [x] `AssetsPage.vue`：刪除後失敗警示
- [x] `AssetImportDialog.vue`：步驟③ `managed_reservations > 0` 提示「請執行 Kea 完整同步」
- [x] `pnpm lint:check`、typecheck、build 全綠

## Comments

實作完成（主實作 commit `f541296`）。

- `AssetFormDialog.vue` `syncInterfaces`：收集介面更新／刪除的 `kea_sync`，多筆失敗僅以 `notifyKeaSync` 提示一次；`deleteCreatedAsset` 於刪除成功後同樣提示。
- `AssetsPage.vue` 刪除流程：`deleteAsset` 回應 `kea_sync` 失敗時警示。
- `AssetImportDialog.vue`：步驟③新增 `bg-info` banner（icon `sync`），`created.managed_reservations > 0` 時顯示筆數並提示至「Kea 同步」執行完整同步。
- 驗證：`pnpm lint:check`、`pnpm --filter frontend typecheck`、`pnpm build:frontend` 全綠。
