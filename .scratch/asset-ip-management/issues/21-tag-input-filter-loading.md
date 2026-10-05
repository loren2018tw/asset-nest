# 21: 資產編輯標籤輸入：既有標籤建議不顯示（loading 轉圈不止）

**What to build:** 修正資產編輯對話框的標籤欄位：輸入時右側 spinner 一直轉、既有標籤建議不顯示。

**Blocked by:** None (can start immediately)

**Status:** done

- [x] 標籤 `q-select` 的 `@filter` 呼叫 `update()`，結束 loading 並開啟選單
- [x] 輸入文字可即時過濾既有標籤、排除已選；新標籤仍可 Enter 新增
- [x] typecheck／lint／build 全綠

## Comments

根因與修正：

- Quasar `q-select` 使用 `@filter`（onFilter）時，`filter()` 會設內部 `innerLoadingIndicator`，並 emit `filter(val, update, abort)`；只有呼叫 `update()` 才會結束 loading 並開啟選單。`AssetFormDialog.filterTags(input)` 只更新 `tagOptions`、未呼叫 `update()`，因此 spinner 不止、建議清單不開。
- `frontend/src/components/AssetFormDialog.vue`：`filterTags` 改為 `(input, update)` 並以 `update(() => {...})` 指派過濾結果（比照 `AssignmentDialog.onFilterAssets`、`AssetsPage.filterLocations` 既有寫法）。
- 資產清單頁的標籤篩選未使用 `@filter`，不受影響。
- 驗收：`pnpm --filter frontend typecheck`、`pnpm --filter frontend lint:check`、`pnpm --filter frontend build` 全綠。
- 實作 commit：`8149c3b`（`21 資產編輯標籤輸入：修正 @filter 未呼叫 update 導致轉圈與建議不顯示（前端）`）
