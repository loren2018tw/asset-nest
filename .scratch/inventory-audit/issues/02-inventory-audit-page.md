# 02: 前端清冊稽核頁——上傳、摘要與兩張差異表

**What to build:** 側欄「資產管理」新增「清冊稽核」（路由 `/inventory-audit`）。上傳區（QUploader 多檔、`.xls,.xlsx`、關閉自動上傳、說明、選檔可移除、「開始稽核」）→ 結果（摘要 chips、逐檔摘要、兩張 q-table：欄位與操作依 spec §6、關鍵字篩選、虛擬滾動、CSV 下載、「重新上傳」）；檔級錯誤以頁面區塊＋`$q.notify` 呈現。CSV 前端產生（UTF-8 BOM、CRLF、RFC 4180；檔名依 spec §6）。「系統有、清冊沒有」提供「在資產清單檢視」連到 `/assets?q=<編號原文>`；`AssetsPage` 支援 `route.query.q` 初始化搜尋。

**Blocked by:** 01

**Status:** done

- [x] `frontend/src/api/inventoryAudit.ts`：型別＋`auditInventory(files: File[])`（FormData、`file` 多值；沿用既有錯誤解析並顯示 400 `details.files`）
- [x] `frontend/src/pages/InventoryAuditPage.vue`：上傳／結果／錯誤三態；整體與逐檔摘要；兩表（含 `duplicate` 標示、篩選、虛擬滾動、空狀態）
- [x] CSV 匯出 util（BOM／CRLF／escape；欄位同表格）
- [x] `routes.ts`＋`MainLayout.vue` 側欄（資產管理區、icon `fact_check`、active 判定）
- [x] `AssetsPage`：`route.query.q` 初始化搜尋字串
- [x] `pnpm --filter frontend typecheck`、`pnpm lint:check`、`pnpm build:frontend` 全綠

## Comments

### 2026-10-09（票 02 實作）

**變更檔案**

- `frontend/src/api/client.ts`：新增 `apiUploadMany<T>(path, files)`（同名欄位 `file` 依序附加；錯誤解析／204／JSON 行為沿用 `send`）；`apiUpload` 改為委派 `apiUploadMany(path, [file])`，行為不變。
- `frontend/src/api/inventoryAudit.ts`（新）：`InventoryAuditKind`／`InventoryAuditFileSummary`／`InventoryAuditSummary`／`InventoryAuditMissingInSystemRow`／`InventoryAuditMissingInListRow`／`InventoryAuditReport`（可空欄位 `string | null`）；`auditInventory(files)`；`fileErrorsOf(details)`（防禦性解析 `details.files`，結構不符回空陣列）。
- `frontend/src/utils/inventoryAudit.ts`（新）：`kindLabel`、`buildMissingInSystemCsv`／`buildMissingInListCsv`（RFC 4180、CRLF、UTF-8 BOM、檔名 `清冊稽核_系統缺少_YYYYMMDD.csv`／`清冊稽核_清冊缺少_YYYYMMDD.csv`）、`downloadMissingInSystemCsv`／`downloadMissingInListCsv`。
- `frontend/src/pages/InventoryAuditPage.vue`（新）：上傳（QUploader `multiple`／`accept=".xls,.xlsx"`／`:auto-upload="false"`／`hide-upload-btn`、說明、已選摘要、「開始稽核」）／結果（摘要 chips、逐檔摘要、兩張虛擬滾動 q-table、篩選、CSV、`重新上傳`）；檔級錯誤頁面區塊＋`$q.notify`。
- `frontend/src/router/routes.ts`：MainLayout 子路由 `inventory-audit`。
- `frontend/src/layouts/MainLayout.vue`：側欄「資產管理」區塊（資產借還之後）新增「清冊稽核」（icon `fact_check`）；`inventoryAuditActive`＝`/inventory-audit` 前綴。
- `frontend/src/pages/AssetsPage.vue`：`onMounted` 以 `route.query.q`（字串且非空）初始化搜尋字串；其餘行為不變。

**關鍵決定／假設**

- 檔級錯誤：頂層 `message`＋`details.files` 逐檔原因同時顯示於頁面 `q-banner`（negative）並 `$q.notify`；非 `ApiError`（網路錯誤等）或無 `details` 時只顯示 `message`。錯誤後保留 uploader 與已選檔案，修正／移除後可直接重送。
- 已選檔案：以 QUploader 內建佇列呈現檔名、大小與移除鈕（比照 AssetImportDialog 慣例），另以 `@added`／`@removed` 同步 `selectedFiles` 作為送出內容；非 `.xls`／`.xlsx` 由 `accept`＋`@rejected` 警告提示。
- 前端不另設 5 MB／10 檔上限（後端為單一權威、整批拒絕＋逐檔回報，見 spec §4）；「開始稽核」無檔停用、請求中 loading。
- 「重新上傳」切回上傳階段並重建 QUploader（`v-if`），佇列與結果一併清空。
- 篩選為前端子字串、不分大小寫；haystack＝該表所有顯示欄位原文（含換行）；筆數標示「共 N 筆」／「顯示 X／共 N 筆」。
- **CSV 匯出「目前顯示列」（含篩選）**：spec §6 未明講範圍，暫以「所見即所下載」實作；若驗收認為應一律全量，只需改 `downloadMissingInSystem`／`downloadMissingInList` 傳入來源改為未篩選清單。「操作」欄不入 CSV、同鍵重複以「是」／空、null→空。
- `property_no` 以 `white-space: pre-wrap` 呈現原文換行（不折疊、不清洗）；其餘可空欄位空值顯示「—」（CSV 仍為空）。
- `AssetsPage` 僅在 mount 時讀取一次 `?q=`（單向初始化，不做 query 雙向同步）。

**指令與結果**

- `pnpm --filter frontend lint`（oxfmt＋oxlint --fix）→ 套用。
- `pnpm --filter frontend typecheck` → 通過（無輸出）。
- `pnpm lint:check` → `oxfmt --check`：All matched files use the correct format；`oxlint` 無警告。
- `pnpm build:frontend` → Build succeeded；產物含 `assets/InventoryAuditPage-*.js`（12.75 KB）。
- 後端煙霧測試（`127.0.0.1:18099`、temp DB `/tmp/opencode/audit-smoke.db`、`AUTH_USERNAME`／`AUTH_PASSWORD` 覆寫；測畢已停並清理，未動 `asset-nest.db`、未用 8080）：
  - `property.xls`＋`items.xlsx` → 200；頂層鍵（`files`／`summary`／`missing_in_system`／`missing_in_list`）與兩表列鍵、型別（`kind` `property|item`、`note` null／string、`duplicate` bool、`asset_id` int）與 TS 完全一致。
  - 非 Excel 檔 → 400：`validation_error`＋中文 `message`＋`details.field="file"`＋`details.files[{filename,message}]`（與 `fileErrorsOf` 預期一致）。
  - 11 檔 → 400：`message`「一次最多上傳 10 個檔案，整批不稽核」、`details.files` 11 筆逐檔「超過單次 10 個檔案上限」。

**已知限制**

- 本 repo 無前端單元測試框架；以 typecheck／lint／build＋真 API 形狀比對驗證，UI 互動留 Ticket 03 手動驗證。
- 已選檔案清單即 QUploader 內建清單；同一批選擇「同名、同大小、同修改時間」的檔案會被 Quasar 靜默視為重複略過（無提示）——實務單檔來源下不影響。
- 檔案摘要列 key＝檔名＋序號；同批次同名檔仍可正確呈現。
- CSV 檔名不含篩選關鍵字（與 spec §6 一致）。

**待 Ticket 03 手動驗證清單**

1. 側欄「資產管理」區塊出現「清冊稽核」（`fact_check`），`/inventory-audit` 與子路徑前綴高亮；未登入導向登入頁。
2. 上傳互動：多選與拖放、非 `.xls/.xlsx` 拒收警告、佇列顯示檔名＋大小、可移除、無檔時「開始稽核」停用、請求中 loading。
3. 成功結果：摘要 chips（清冊合計／系統資產／相符／系統缺少／清冊缺少）、逐檔摘要（檔名、財產／物品 badge、資料列、略過）、兩表欄位與內容、`同鍵重複`、關鍵字篩選與筆數、虛擬滾動、無差異／無命中空狀態。
4. 錯誤：>10 檔、>5 MB、壞檔／非 Excel → 頁面錯誤區塊逐檔＋negative notify，且不顯示結果。
5. CSV：BOM／CRLF、欄位同表格（操作欄除外）、同鍵重複「是」／空、null→空、中文檔名；並確認「篩選後下載」範圍是否符合驗收期待（見假設）。
6. 「在資產清單檢視」→ `/assets?q=<編號原文>`，AssetsPage 搜尋框帶入、清單已依該字串查詢。
7. 「重新上傳」後佇列清空、回到上傳狀態；失敗後修正檔案可重送成功。
