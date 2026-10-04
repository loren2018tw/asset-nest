# 11: 資產標籤、標籤篩選與清單排序

**What to build:** 資產新增多值「標籤」欄位（自由文字、可多個）；資產清單顯示標籤欄位、新增標籤篩選下拉；資產清單標題列可點擊快速排序（伺服器端）。因系統未部署：直接修改 baseline migration `0002_assets.sql`（不新增 migration 檔），並手動更新本機 `asset-nest.db`（ALTER TABLE＋同步 sqlx 檢查碼），保留現有資料。

**Blocked by:** None (can start immediately)

**Status:** done

- [x] assets 新增 tags 欄位（多值、自由文字；存 JSON 陣列、預設 `[]`）；baseline `0002_assets.sql` 直接修改，本機 DB 以 ALTER TABLE 更新並同步 `_sqlx_migrations` 檢查碼，現有資料保留
- [x] 資產建立／編輯可輸入多個標籤（既有標籤建議、可輸入新標籤；trim、忽略空字串、不分大小寫去重）
- [x] 資產清單顯示標籤欄位（chips）
- [x] 資產清單標籤篩選下拉（列出所有已使用標籤、不分大小寫完全符合；伺服器端篩選）
- [x] 資產清單標題列點擊排序（伺服器端排序；欄位白名單；預設描述升冪；無效參數有明確錯誤）
- [x] 後端整合測試涵蓋標籤 CRUD／驗證／篩選／排序與 GET /tags 去重

## Comments

實作完成（commit 訊息：`11 資產標籤與清單排序：多值標籤、標籤篩選、伺服器端排序（後端＋前端）`）。

- 資料庫（未部署階段，baseline 直接修改＋本機 DB 手動同步）：
  - `backend/migrations/0002_assets.sql`：`assets` 新增 `tags TEXT NOT NULL DEFAULT '[]'`（放 `note` 之後）；不新增 migration 檔。
  - 本機 `asset-nest.db`（已確認無後端行程在跑）：以 `python3` 的 `sqlite3` 執行 `ALTER TABLE assets ADD COLUMN tags TEXT NOT NULL DEFAULT '[]'`（欄位實際附加於 `updated_at` 後；程式一律用明確欄位清單，不依賴順序）；再以 `hashlib.sha384(檔案內容)` 計算新檢查碼並 `UPDATE _sqlx_migrations SET checksum = ? WHERE version = 2`。
  - 驗證：以預設 `DATABASE_URL`（`sqlite://asset-nest.db`）啟動後端，`GET /api/health` 回 `{"status":"ok",...,"database":"ok"}`、migrations 無錯誤；`SELECT tags FROM assets` 欄位存在；現有 3 筆資產完整保留（`tags = '[]'`）。驗證後已停止該 dev server（使用者可自行重啟）。
- 後端：
  - `backend/src/assets.rs`：`Asset` 加 `tags: Vec<String>`（DB 存 JSON 字串）；`AssetInput.tags: Option<Vec<String>>`（建立預設空）；`AssetPatch.tags` 比照 `double_option`（未提供＝不變、`null`＝清空、陣列＝設定）；`normalize_tags` 逐項 trim、忽略空字串、不分大小寫去重（保留首次出現原樣，大小寫折疊比照 SQLite `COLLATE NOCASE` 僅 ASCII，與篩選一致）。
  - `GET /assets` 新增查詢參數：`tag` 以 `EXISTS (SELECT 1 FROM json_each(assets.tags) WHERE value = ? COLLATE NOCASE)` 做不分大小寫完全符合；`sort`／`dir` 以白名單（`property_no`／`description`／`location`／`brand`／`model`／`note`／`tags`／`expired`；`asc`／`desc`）驗證，無效值回 400 `validation_error`（`details.field` 為 `sort`／`dir`）；文字欄位 `COLLATE NOCASE`，`expired` 以 SQL CASE 運算式排序（`date('now','localtime')` 對齊 Rust `Local::now().date_naive()`；`+N years` 對 2/29 會進位 3/1，已特別修正為 2/28 與 Rust 一致）；一律加 `id ASC` 決勝鍵；未提供時維持預設 `description COLLATE NOCASE ASC, id ASC`。
  - 新增 `GET /api/v1/tags`：以 `json_each` 列出所有已使用標籤，不分大小寫去重、依 NOCASE 排序，保留最早寫入原文；回應形狀同 `/locations`、`/brands`（`{items:[...]}`）。
- 前端：
  - `frontend/src/api/assets.ts`：`Asset`／`AssetInput` 加 `tags: string[]`；`AssetListParams` 加 `tag`／`sort`／`dir`；新增 `fetchTags()`。
  - `AssetsPage.vue`：新增「標籤」欄（q-chip）；所有資料欄 `sortable: true`（操作欄除外）；q-table `@request` 的 `sortBy`／`descending` 傳後端，初始 `description` 升冪，切換排序時回到第 1 頁；新增標籤篩選 q-select（options 來自 `fetchTags()`、clearable、變更即重載）；搜尋框的 `filters.q: string | null` 與 `?.trim()` 維持不動。
  - `AssetFormDialog.vue`：標籤輸入用 q-select（`multiple`、`use-input`、`use-chips`、`new-value-mode="add-unique"`），options 為既有標籤建議（排除已選、依輸入過濾），編輯時載入既有 tags、建立／編輯皆隨資產送出。
- 測試：
  - `backend/tests/assets.rs` 新增 5 個整合測試：標籤建立／編輯／清空／trim／不分大小寫去重、tag 篩選（大小寫無關、精確不子字串、可組合）、排序各欄位 asc／desc（含 NULL 與預設值）、`sort=expired` 與 Rust 旗標一致性（含時區邊界與 2/29）、無效 sort／dir 400、`GET /tags` 去重排序；既有 CRUD 測試補 `tags` 預設斷言。
  - 驗收：`cargo test`（45 單元＋所有整合全綠）、`cargo fmt --check`、`pnpm typecheck`、`pnpm lint:check`、`pnpm build` 全綠；另以真實伺服器＋本機 DB smoke：建立含標籤資產（`[" Smoke ","smoke","ZZ"]`→`["Smoke","ZZ"]`）、tag 篩選、`sort=expired&dir=desc`、`GET /tags`、無效 sort 400，並清理臨時資產。
- 設計取捨：
  - `tags` 排序以原始 JSON 字串比較（`tags COLLATE NOCASE`）：空陣列 `[]` 依字串序排在 `["a"]` 之後；行為確定、可測，未另做「取首個標籤」等特殊語意。
  - 標籤大小寫折疊沿用全系統 SQLite `COLLATE NOCASE` 的 ASCII 語意（Rust 端用 `to_ascii_lowercase`），非 ASCII 大小寫視為不同值。
  - `expired` 排序以 SQL 重算而非新增資料欄位；已以整合測試驗證排序與 Rust 屆齡旗標一致。
- 實作 commit：`1a1c185`（`11 資產標籤與清單排序：多值標籤、標籤篩選、伺服器端排序（後端＋前端）`）
