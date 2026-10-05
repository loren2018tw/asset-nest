# 22: 資產編輯標籤輸入：點選既有標籤後清空原輸入文字

**What to build:** 標籤欄位輸入部分文字後，點選建議的既有標籤，原輸入文字應清空，方便連續輸入。

**Blocked by:** None (can start immediately)

**Status:** done

- [x] 點選既有標籤後清空輸入文字；選單維持開啟、剛選取的標籤自建議清單移除
- [x] Enter 新增新標籤路徑不受影響
- [x] typecheck／lint／build 全綠

## Comments

根因與修正：

- Quasar `q-select`（`multiple` + `use-input`）的滑鼠點選路徑 `toggleOption()` 只更新 model、不清空 `inputValue`（鍵盤 Enter 路徑會）；因此輸入部分文字後點選建議標籤，原文字殘留。
- `frontend/src/components/AssetFormDialog.vue`：`q-select` 加 `ref` 與 `@add`；`onTagAdded` 於 `nextTick` 後呼叫公開方法 `updateInputValue("", false)` 清空輸入並重跑過濾（待 model 更新後才過濾，剛選取的標籤才會自清單排除；`false` 觸發 filter 讓選單維持開啟，可連續點選）。
- 驗收：`pnpm --filter frontend typecheck`、`pnpm --filter frontend lint:check`、`pnpm --filter frontend build` 全綠。
- 實作 commit：`87e7428`（`22 資產編輯標籤輸入：點選既有標籤後清空原輸入文字（前端）`）
