# 03: 前端匯入對話框——選檔 → 預覽 → 匯入結果

**What to build:** 資產清單工具列新增「匯入」按鈕，開啟三步對話框：①選檔（拖放、關閉自動上傳、說明文案、下載範本、無網段警示）→ 以 `dry_run=true` 預覽；②預覽（摘要 chips、偵測編碼、忽略欄位提示、全 14 欄表格＋虛擬滾動、「只看問題列」切換、問題列報告下載；有錯誤時停用匯入、僅警示時先確認）；③結果（建立數量與警示數、報告下載、完成後重載清單與篩選選項）。範本內容須與 `docs/資產匯入範本.csv` 一致；問題列報告為 UTF-8 BOM CSV（列號＋14 欄原始值＋原因，多條以「；」串接）。

**Blocked by:** 02

**Status:** done

- [x] API client 支援 multipart 上傳（FormData；沿用既有 `{error,message,details}` 錯誤解析）
- [x] 工具列「匯入」按鈕與對話框三步流程（可回上一步、可重新選檔）
- [x] 選檔步驟：接受 `.csv`、`.xlsx` 提示不支援；說明（編碼／標籤 `|` 分隔／日期格式／需先建網段／全有全無）；「下載範本」按鈕；系統無網段時顯示警示橫幅
- [x] 預覽步驟：summary 與 encoding、ignored_headers 顯示；表格含列號＋14 欄＋狀態＋原因、虛擬滾動、「只看問題列」；問題列報告下載；錯誤 > 0 時「匯入」停用；警示 > 0 時先跳確認對話框
- [x] 結果步驟：顯示 created 統計與警示數；完成後重載清單與位置／廠牌／標籤選項
- [ ] 手動 E2E：以範本檔改資料匯入成功（清單、資產詳情、IP 頁可見 v4 保留／手動與 v6 指派）；含錯誤的檔案整批不匯入；問題列報告可下載且列號對得上
- [x] `pnpm typecheck`、`pnpm lint:check`、`pnpm build` 全綠

## Comments

2026-10-04 實作完成（commit `ef46fc4`）。

### 變更

- `frontend/src/api/client.ts`：新增 `apiUpload<T>(path, file)`（FormData、檔案欄位 `file`；不手動設 `Content-Type`，由瀏覽器帶 boundary）。`request` 重構為共用 `send`（錯誤解析／204／JSON 行為不變），既有函式不受影響。
- `frontend/src/api/assets.ts`：新增 `ImportIssue`、`ImportRowData`（14 欄）、`ImportRowStatus`、`ImportRow`、`ImportSummary`、`ImportCreated`、`ImportReport` 型別與 `importAssets(file, dryRun)`（`/api/v1/assets/import?dry_run=…`）。
- `frontend/src/utils/assetImport.ts`（新）：範本下載（UTF-8 BOM）、問題列報告 CSV（標題＝列號＋14 欄＋原因；原因以「；」串接、RFC 4180 escape、CRLF）、檔名 `匯入問題報告_YYYYMMDD.csv`。
- `frontend/src/assets/資產匯入範本.csv`（新）：與 `docs/資產匯入範本.csv` 位元組相同（`cmp` 一致），以 Vite `?raw` 匯入；補 `frontend/src/shims-raw.d.ts` 型別宣告。
- `frontend/src/components/AssetImportDialog.vue`（新）：q-dialog（寬 1400px／高 92vh、persistent）自製三步狀態：
  - ①選檔：QUploader（`auto-upload=false`、`accept=".csv"`、`hide-upload-btn`）、`.xlsx` 拒收提示、說明清單（編碼／14 欄標題／標籤 `|`／日期格式／需先建網段／全有全無）、下載範本；開對話框讀 `GET /subnets`，為空顯示警示橫幅；「預覽」帶 loading。
  - ②預覽：摘要 chips（總筆數／OK／警示／錯誤／偵測編碼）、`ignored_headers` 提示、錯誤列整批提示；q-table 虛擬滾動（列號＋14 欄＋狀態 badge＋原因）固定高度、「只看問題列」、問題列報告下載；「上一步」「重新選檔」；「匯入 M 筆」（M＝total－errors）有錯誤停用並提示、僅警示先跳 `$q.dialog` 確認。
  - ③結果：created（資產／介面／指派）與警示數、問題列報告下載、「完成」→ emit `saved` 並關閉。
  - `committed=false`（預覽後資料變動）：以回應更新預覽 rows／summary，`$q.notify`「資料已變動，未匯入，請確認後再試」。
  - 狀態顏色：OK＝positive（綠）、警示＝warning（黃）、錯誤＝negative（紅）。
- `frontend/src/pages/AssetsPage.vue`：「新增資產」左側加「匯入」按鈕（icon `upload_file`）；`<asset-import-dialog v-model="importOpen" @saved="onSaved" />`，`onSaved` 已重載清單與位置／廠牌／標籤選項。

### 執行過的指令與結果

- `pnpm --filter frontend typecheck` → 綠（vue-tsc 無錯誤）。
- `pnpm lint:check` → 綠（oxfmt＋oxlint）。
- `pnpm build:frontend` → Build succeeded（SPA；未動後端 release）。建置產物中的範本字串與 `docs/資產匯入範本.csv` 完全相同（含 BOM、160 字元）。
- 以 Vite SSR 載入真實前端模組（臨時腳本，未入庫）驗證：
  - `importAssets`：body 為 FormData、檔案欄位 `file`、URL 帶 `dry_run`、headers 僅 `Accept`（無 Content-Type）；400 時 `ApiError` 的 message／details 正確。
  - `buildIssueReportCsv`：只含問題列、escape（逗號／引號／換行）、標籤以 `|`、原因以「；」、列號＝`row_number`；檔名格式正確。
- 真實後端整合（temp DB、port 18080，臨時腳本，未入庫）：以 `docs/資產匯入範本.csv` 走前端 `importAssets`——無網段預覽回 `ipv4_out_of_subnet` 且問題報告列號為 2；建立 `10.0.0.0/24` 後列 ok、未知欄位回 `ignored_headers=["數量"]`；`dry_run=false` 回 `committed=true`、`created={assets:1,interfaces:1,assignments:1}`，清單可見且詳情為 `eth0`＋保留 10.0.0.10／hostname `pc-001`；同檔再次匯入回 `committed=false`、`created=null`、`ipv4_assigned`（對應 UI 資料變動路徑）。

### 假設與已知限制

- 問題列報告的 14 欄值取自回應 `data`（後端正規化後；無法正規化者為 null，欄數不一致列保留原值），非原始檔案字元——後端回應未提供無效欄位的原始輸入（見票 02）。
- 未新增任何 npm 依賴。

### 待確認

- 瀏覽器手動 E2E 待使用者確認：拖放／選檔、三步按鈕流（上一步／重新選檔／完成）、虛擬滾動、「只看問題列」、範本與問題報告下載、無網段橫幅、僅警示時的確認對話框。
