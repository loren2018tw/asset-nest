# 03: 網段匯入對話框

**What to build:** 網段設定頁「匯入」開啟三步對話框（選檔→預覽→結果），流程比照資產匯入對話框。

**Blocked by:** 02 網段匯入後端

**Status:** done

- [x] 選檔（`.csv`，關閉自動上傳）＋說明（編碼、pool 格式、只新增、全有全無）；預覽以 `dry_run=true` 呼叫
- [x] 預覽：摘要 chips（總數／OK／警示／錯誤／編碼）、忽略欄位提示、逐列表格（列號＋6 欄＋狀態＋原因）、「只看問題列」切換；有錯誤列時停用匯入
- [x] 問題列報告下載（`網段匯入問題報告_YYYYMMDD.csv`，UTF-8 BOM）
- [x] 結果：建立網段數與「完成」→ 重載網段列表
- [x] 前端 typecheck／lint 全綠

## Comments

實作完成（commit `497c601`）。

- 新增 `frontend/src/components/SubnetImportDialog.vue`：比照 `AssetImportDialog` 的三步對話框（選檔→預覽→結果）；QUploader 限定 `.csv`（非 `.csv` 提示另存）且關閉自動上傳；選檔說明含編碼（UTF-8 BOM／Big5）、6 欄標題與未知欄位忽略、Gateway 在 CIDR 內、Kea subnet-id 僅 v4 且全系統唯一、pool 以 `|` 分隔且僅 v4、只新增與全有全無。
- 預覽以 `importSubnets(file, true)`（`dry_run=true`）呼叫：摘要 chips（總筆數／OK／警示／錯誤／偵測編碼）、忽略欄位提示、錯誤列橫幅、逐列表格（列號＋名稱／CIDR／Gateway／Kea subnet-id／位址池／備註＋狀態＋原因）、「只看問題列」切換；有錯誤列時停用匯入；僅有警示時先跳確認（比照資產匯入；本匯入 warnings 恆 0）。
- `frontend/src/api/subnets.ts` 新增 `importSubnets(file, dryRun)`（multipart、欄位 `file`，沿用 `apiUpload`）與 `SubnetImport*` 型別（`encoding`／`ignored_headers`／`summary`／`rows`／`committed`／`created={subnets}`；`data` 為 `name`／`cidr`／`gateway`／`kea_subnet_id`／`pools`／`note`）。
- 新增 `frontend/src/utils/subnetImport.ts`：6 欄問題列報告（列號＋6 欄原始值＋原因多條以「；」串接）、RFC 4180 引號規則、UTF-8 BOM，檔名 `網段匯入問題報告_YYYYMMDD.csv`；下載沿用 `api/client` 的 `saveBlob`。
- `frontend/src/pages/SubnetsPage.vue`：工具列「匯出」旁新增「匯入」（icon `upload_file`）；`subnet-import-dialog` 的 `@saved` 沿用 `onSaved` 重載網段列表。
- 正式匯入 `committed=false`（預覽後資料變動）時以重驗結果更新預覽並提示重試，比照資產匯入；成功才進結果步驟（建立網段數＋警示 chip＋完成）。
- 取捨：未提供範本下載（spec 未要求，無對應範本檔）；匯入型別獨立於資產匯入（`data` 6 欄、`created={subnets}` 形狀不同），避免 `assets.ts`／`subnets.ts` 跨模組耦合；問題列報告輔助函式與 `assetImport.ts` 平行獨立，未動既有檔案。
- 驗收：`pnpm --filter frontend typecheck`、`pnpm lint:check` 全綠；`pnpm test` 全綠（195 passed／17 suites、0 failed），無回歸。
- 實作 commit：`497c601`（`03 網段匯入對話框：選檔→預覽→匯入（前端）`）
