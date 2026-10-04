# 02: 網段匯入後端

**What to build:** `POST /subnets/import` 接受 CSV（UTF-8／Big5），逐列驗證後以單一交易新增網段；dry-run 提供預覽報告。

**Blocked by:** None (can start immediately)

**Status:** done

- [x] 檔級規則沿用資產匯入：標題列處理、編碼偵測（回應 `encoding`）、大小／列數上限、傳輸層錯誤 400
- [x] 列級結構錯誤（任一即整批不寫入）：CIDR 格式／已存在（含檔內重複）／與既有或檔內重疊（含嵌套）、gateway 不在 CIDR 內、Kea subnet-id 非正整數或重複、pool 格式／範圍／重疊錯誤、v6 帶 Kea subnet-id 或 pool
- [x] 只新增：不更新、不刪除既有網段
- [x] `dry_run=true` 回預覽報告（比照資產匯入：`encoding`／`ignored_headers`／`summary`／`rows`）；`dry_run=false` 成功回 `committed=true`、`created={subnets}`，於單一交易完成
- [x] 領域寫入可於交易內重用（必要時先做最小前置重構；比照資產匯入的前置重構）
- [x] 後端整合測試涵蓋各結構規則、檔內重複、v6 限制、dry-run 不寫入、正式匯入原子性

## Comments

實作完成（commit `610b4be`）。

- 新增 `backend/src/subnet_import.rs`（領域邏輯：編碼偵測、CSV 解析、逐列驗證、跨列 CIDR／Kea 比對、單一交易寫入）與 `backend/src/api/subnet_import.rs`（multipart 路由；`DefaultBodyLimit::disable()`＋端點自行檢查 5 MB，超限一律 400 而非 413）；靜態路徑 `/subnets/import` 與 `/subnets/{id}`、`/subnets/export` 並存。
- 前置重構（同票）：`subnets::create` 抽出 `insert_subnet(&mut SqliteConnection, ValidSubnet)` 原語（網段＋pools 於同一連線），`create` 改為薄包裝並維持單一交易；`insert_pools` 改收連線；`normalize_cidr` 開放 `pub(crate)` 供匯入重用正規化與地址族判斷。既有 API 行為不變。
- 驗證重用：欄位規則走 `SubnetInput::validate`（gateway 在 CIDR 內、pool 格式／範圍／段間重疊、v6 限制；單一驗證權威）；匯入特有的跨列規則（CIDR 已存在／與既有或檔內重疊、Kea subnet-id 重複）於 `subnet_import` 比對既有資料與檔內較前列。無語意警示，`summary.warnings` 恆 0。
- 檔級規則沿用資產匯入：UTF-8 BOM→嚴格 UTF-8→Big5（回應 `encoding`）、標題 trim／英文不分大小寫／順序不拘、未知標題忽略並列 `ignored_headers`、重複標題與缺 `CIDR` 標題 400、5 MB／5,000 列上限、RFC 4180、空行忽略、傳輸層錯誤 400 `{error,message}`。
- 列級錯誤代碼：`cidr_required`／`invalid_cidr`／`cidr_duplicate`／`cidr_overlap`／`invalid_gateway`／`invalid_kea_subnet_id`／`kea_subnet_id_for_v6`／`kea_subnet_id_duplicate`／`invalid_pools`／`pools_for_v6`／`column_count_mismatch`；`data` 為 `name`／`cidr`（正規化）／`gateway`／`kea_subnet_id`／`pools`（`["起點-終點", …]`）／`note`。
- 正式匯入以當下資料重驗，全數通過才於單一交易寫入；成功回 `committed=true`＋`created{subnets}`，有結構錯誤回 HTTP 200、`committed=false`、`created=null`；唯一性衝突（預覽後他人異動）為 400 雙保險。
- 測試：`backend/tests/subnet_import.rs` 14 個整合測試（檔級錯誤、編碼、CIDR 各規則、gateway、Kea、pool、dry-run 不寫入、成功匯入含 pools 落地、原子性、預覽後資料變動）；`subnet_import` 模組 10 個單元測試＋`subnets` 模組新增 2 個交易原語測試（同交易可見、回滾不留資料）。
- 取捨：pool「檔內不重疊」由 CIDR 不重疊規則涵蓋（合法檔案中不同網段的 pool 不可能重疊），未另立跨列 pool 檢查；CIDR 重疊以線性掃描檔內已登錄網段（列數上限 5,000）。
- 驗收：`pnpm test` 全綠（85 單元＋110 整合、0 failed）；`cargo fmt --check` 通過。以暫存 SQLite＋真實伺服器 smoke：預覽、commit（`created{subnets:2}`）、匯出後重新匯入（如預期全部 `cidr_duplicate`／`kea_subnet_id_duplicate` 擋下）。
- 實作 commit：`610b4be`（`02 網段匯入後端：解析、驗證、dry-run 與單一交易寫入（後端）`）
