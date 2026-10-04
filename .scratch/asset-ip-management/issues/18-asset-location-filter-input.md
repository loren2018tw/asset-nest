# 18: 資產管理位置篩選可輸入過濾

**What to build:** 資產管理頁的「位置」篩選下拉可輸入文字，即時過濾既有位置選項後再點選。

**Blocked by:** None (can start immediately)

**Status:** done

- [x] 「位置」篩選改為可輸入過濾（`use-input`，本地過濾既有位置清單；輸入不觸發伺服器查詢）
- [x] 選取後行為與現在相同（伺服器端篩選）；清除（clearable）行為不變
- [x] 廠牌、標籤兩個篩選維持現狀
- [x] 前端 typecheck／lint 全綠

## Comments

實作完成（commit 訊息：`18 資產管理位置篩選可輸入：本地過濾選項（前端）`）。

- 前端：
  - `frontend/src/pages/AssetsPage.vue`：位置 `q-select` 加上 `use-input` 與 `@filter="filterLocations"`；`@update:model-value="reload"` 維持不變，僅選取／清除時觸發伺服器查詢。
  - 新增 `allLocationOptions`（`GET /locations` 載入的完整清單）作為本地過濾來源；`locationOptions` 改為過濾後顯示清單，`loadFilterOptions` 同時更新兩者（存檔／刪除後刷新時回復完整清單）。
  - `filterLocations` 以 `update()` 回呼對完整清單做大小寫無關子字串過濾；輸入過程不呼叫 API。
- 驗收：
  - `pnpm --filter frontend typecheck`、`pnpm lint:check` 全綠。
  - `pnpm test`（70 單元＋95 整合，共 165）全綠；本票無後端程式變更。
- 取捨：
  - 過濾語意與表單標籤建議一致（`toLowerCase().includes()`、不 trim），輸入即時反映。
  - 廠牌、標籤篩選與清單排序／分頁行為未動。
- 實作 commit：`89233cc`（`18 資產管理位置篩選可輸入：本地過濾選項（前端）`）
