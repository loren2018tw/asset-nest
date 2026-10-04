# 04: 資產匯出

**What to build:** 資產清單可將目前搜尋／篩選／排序結果輸出成與匯入範本相同的 14 欄 CSV。

**Blocked by:** None (can start immediately)

**Status:** done

- [x] `GET /assets/export` 同清單查詢參數（q／location／brand／device_serial／tag／sort／dir；忽略分頁）；回 `text/csv`（attachment）
- [x] 14 欄與 `docs/adr/0008` 一致（標籤以 `|` 串接、日期 `YYYY-MM-DD`、MAC 小寫冒號）；一列一資產
- [x] 介面與位址選取規則依 spec：有指派位址的介面中 id 最小者；v4／v6 各取該介面數值最小者；hostname 僅當匯出的 IPv4 為 reservation（其餘空）
- [x] 資產工具列新增「匯出」按鈕（檔名 `資產匯出_YYYYMMDD.csv`）；UTF-8 BOM
- [x] 後端整合測試涵蓋：篩選條件套用、多介面／多 IP 選取規則、無介面、標籤與日期格式

## Comments

實作完成（commit 訊息：`04 資產匯出：CSV 端點與匯出按鈕（後端＋前端）`）。

- 後端：
  - `backend/src/assets.rs`：新增 `list_all`（套用與清單相同的 `push_filters` 與排序、不分頁；排序子句抽出 `push_order` 與 `list` 共用）。
  - `backend/src/assignments.rs`：新增 `ExportNetwork` 與 `export_networks(pool, asset_ids)`——單一查詢 `interfaces LEFT JOIN ip_assignments` 批次取多資產的介面＋指派（避免 N+1，比照票 12 `list_for_assets`）；`select_export_network` 依 spec §4 選取：有指派位址的介面中 id 最小者（皆無指派→id 最小介面；無介面→全空）、v4／v6 各取該介面數值最小者（u32／u128）、hostname 僅當匯出的 IPv4 為 reservation。
  - 新增 `backend/src/asset_export.rs`：`export_csv`（讀取資產＋網路欄位）與 `to_csv`（UTF-8 BOM＋標題列；14 欄、缺值空字串、標籤以 `|` 串接、日期已是 `YYYY-MM-DD`）。
  - `backend/src/api/assets.rs`：新增 `GET /api/v1/assets/export`（attachment、`text/csv; charset=utf-8`、檔名 `資產匯出_YYYYMMDD.csv` 以 RFC 5987 `filename*` 編碼＋ASCII fallback）；`ListQuery` 抽出 `into_filter` 與清單共用（匯出忽略分頁）；靜態路徑與 `/assets/{id}`、`/assets/import` 並存。
  - `backend/src/api/mod.rs`：`encode_filename` 自 `api/subnets.rs` 移為 `pub(crate)` 共用（票 01 的網段匯出同步改用 import）。
- 前端：
  - `frontend/src/api/assets.ts`：新增 `downloadAssetsCsv`（沿用票 01 的 `apiDownload`）與 `AssetExportParams`；查詢字串組裝抽出 `queryString` 與清單共用。
  - `frontend/src/pages/AssetsPage.vue`：工具列「匯出」按鈕（icon `download`、下載中 loading、失敗以既有 `$q.notify` 顯示）；帶入目前 `filters` 與 `sortBy`／`descending`，後端未帶檔名時以本機日期組 `資產匯出_YYYYMMDD.csv`。
- 測試：
  - `backend/tests/assets.rs`：新增 `export_assets_applies_filters_sort_and_ignores_pagination`（空庫僅標題列、BOM／Content-Type／Content-Disposition、14 欄、q／location＋tag 篩選、排序、`page`／`per_page` 忽略、逗號備註與標籤 `|`、日期格式、完整列）、`export_assets_rejects_invalid_sort_and_dir`、`export_assets_selects_interface_addresses_and_hostname`（無介面、第一介面無指派取第二、同族取數值最小、hostname 僅保留）。
  - `backend/src/assignments.rs`、`backend/src/asset_export.rs`、`backend/src/api/mod.rs` 單元測試。
- 取捨：
  - 匯出組裝放在新 `asset_export` 模組（跨 assets／assignments 的 orchestration，比照 `import.rs` 的定位）；spec §4 的介面／位址選取規則留在 `assignments` 領域模組。
  - HTTP 檔名編碼移到 `api/mod.rs` 共用，網段匯出同步改 import；未新增通用 export 模組。
- 驗收：
  - `pnpm test` 全綠（93 單元＋113 整合，共 206；較前一票 195 ＋14）。
  - `cargo fmt --check`、`pnpm --filter frontend typecheck`、`pnpm lint:check` 全綠。
  - 以暫存 SQLite＋真實伺服器 smoke：`GET /api/v1/assets/export` 的標頭、BOM、14 欄、篩選、排序、忽略分頁、位址與 hostname 選取皆符合 spec §4。
- 實作 commit：`738a408`（`04 資產匯出：CSV 端點與匯出按鈕（後端＋前端）`）
