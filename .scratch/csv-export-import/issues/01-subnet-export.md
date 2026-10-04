# 01: 網段匯出

**What to build:** 網段設定頁可將全部網段設定輸出成 CSV（格式依 `docs/adr/0009`），供空庫重建時匯入。

**Blocked by:** None (can start immediately)

**Status:** done

- [x] `GET /subnets/export` 回 `text/csv`（attachment）：欄位＝名稱／CIDR／Gateway／Kea subnet-id／位址池／備註；僅 v4 有 Kea subnet-id 與 pool（多段以 `|` 分隔、每段 `起點-終點`）
- [x] 排序 v4 先、v6 後，同族依 CIDR 數值；UTF-8 BOM；第一列標題列
- [x] 網段設定工具列新增「匯出」按鈕，點擊即下載（檔名 `網段匯出_YYYYMMDD.csv`）
- [x] 後端整合測試涵蓋：v4 多 pool、v6（兩欄留空）、選填欄位缺值、BOM 與標題列

## Comments

實作完成（commit 訊息：`01 網段匯出：CSV 端點與匯出按鈕（後端＋前端）`）。

- 後端：
  - `backend/src/subnets.rs`：新增 `list_full`（兩筆查詢載入全部網段＋pools，避免逐網段 N+1）與 `export_csv`（UTF-8 BOM、標題列、`compare_networks` 排序 v4 先／v6 後、同族依網路位址數值；v6 的 Kea subnet-id 與位址池一律留空、選填欄位缺值為空字串）。
  - `backend/src/api/subnets.rs`：新增 `GET /api/v1/subnets/export`（attachment、`text/csv; charset=utf-8`）；靜態路徑與 `/subnets/{id}` 並存（同 `/assets/import` 前例，axum 靜態優先）。檔名 `網段匯出_YYYYMMDD.csv` 以 `Local::now()` 組出、RFC 5987 `filename*=UTF-8''…` 編碼，並附 ASCII fallback `filename="subnets_export_YYYYMMDD.csv"`。
- 前端：
  - `frontend/src/api/client.ts`：新增 `apiDownload`（fetch blob、沿用 `{error,message,details}` 錯誤解析、解析 `Content-Disposition` 的 `filename*`／`filename`）與 `saveBlob`（`<a download>` 儲存）。
  - `frontend/src/api/subnets.ts`：新增 `downloadSubnetsCsv()`。
  - `frontend/src/pages/SubnetsPage.vue`：工具列「新增網段」旁新增「匯出」按鈕（icon `download`）；下載中顯示 loading、失敗以通知顯示；後端未帶檔名時以本機日期組 `網段匯出_YYYYMMDD.csv`。
- 測試：
  - `backend/tests/subnets.rs`：新增 `export_subnets_returns_ordered_csv_with_bom`（v4 多 pool＋含逗號備註經引號往返、v4 選填缺值、v6 兩欄留空、排序、BOM、content-type 與 Content-Disposition 含當日日期）。
  - `backend/src/subnets.rs`、`backend/src/api/subnets.rs` 單元測試：CSV 欄位／排序／v6 留空與 `filename*` 編碼。
- 取捨：
  - 排序在記憶體以 `IpNet` 網路位址比較（SQLite 無法直接比較 CIDR 數值）；資料量小，維持單一程式路徑。
  - 匯出檔名同時附 ASCII `filename` fallback；前端優先採用 `filename*`。
  - 未新增共用 export 模組（票 04 資產匯出屆時再評估），本票僅小幅擴充 `subnets` 領域模組。
- 驗收：
  - `pnpm test` 全綠（73 單元＋96 整合，共 169；較前次＋3 單元＋1 整合）。
  - `cargo fmt --check`、`pnpm --filter frontend typecheck`、`pnpm lint:check` 全綠。
  - 以暫存 SQLite 啟動後端 smoke test：`GET /api/v1/subnets/export` 標頭、BOM、欄位與排序符合 spec §2／ADR-0009。
- 實作 commit：`e5a4dc5`（`01 網段匯出：CSV 端點與匯出按鈕（後端＋前端）`）
