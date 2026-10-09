# 01: 後端清冊稽核——Excel 解析、比對與 API

**What to build:** 新增 `POST /api/v1/inventory-audit`（multipart、`file` 欄位可重複 1～10）。以 calamine 直讀 `.xls／.xlsx`：逐工作表掃描「財產編號」表頭（前 10 列、去空白後相等定位）、欄位依表頭文字定位、編號格含數字者為資料列、編號空但名稱有值者計入略過；種類依表頭上方文字（含「物品」→ 物品）。以正規化鍵（去前後空白、中間空白壓縮為單一半形空格、ASCII 不分大小寫）比對系統內「財產編號非空白」的資產，回傳 `files`／`summary`／`missing_in_system`／`missing_in_list`（spec §5.2）。檔級錯誤整批 400、`details.files` 逐檔回報（spec §4）。唯讀：不寫入、不落地。

**Blocked by:** 無

**Status:** done

- [x] 依賴：新增 `calamine`（xls＋xlsx、以記憶體位元組解析）
- [x] `backend/src/inventory_audit.rs`：檔解析（表頭定位、資料列／略過列、種類判定、原文保留）、鍵正規化、比對（相符統計、兩向差異、`duplicate` 標示、鍵升冪排序）
- [x] `backend/src/api/inventory_audit.rs`：multipart 多檔（同名欄位 `file`）、上限（5 MB／5,000 列／10 檔）、檔級錯誤以 `ApiError::validation().field("file").detail("files", …)`；挂進 v1 router
- [x] 單元測試：正規化（換行、連續空白、全形空白、大小寫）、資料列與略過列判定、種類判定、多行編號、同鍵重複
- [x] 整合測試：`.xls` 與 `.xlsx` 合成 fixtures（含多行編號、頁尾列、空白三聯頁；以 LibreOffice 產製、置 `backend/tests/fixtures/`）；多檔合併；兩向差異與原文保留；檔級錯誤逐項（非 Excel、無表頭、0 列、11 檔、超限）；唯讀（呼叫前後 assets 不變）
- [x] `cargo fmt --check`、`cargo test` 全綠

## Comments

### 2026-10-09（票 01 實作）

**變更檔案**

- `backend/Cargo.toml`／`backend/Cargo.lock`：新增 `calamine = "0.36.1"`（預設 features；xls／xlsx 內建、無 chrono）。
- `backend/src/inventory_audit.rs`（新）：`SheetGrid` 文字化網格、表頭定位（前 10 絕對列、去空白相等）、欄位定位（依文字、重複取第一）、資料列／略過列、種類判定、鍵正規化、系統側單一查詢、兩向比對與穩定排序。單元測試 11 個。
- `backend/src/api/inventory_audit.rs`（新）：`POST /api/v1/inventory-audit`；`DefaultBodyLimit::disable()`＋自檢 5 MB；`file` 同名欄位多值；錯誤沿用 `ApiError` 格式。
- `backend/src/api/mod.rs`、`backend/src/lib.rs`：掛載模組與 v1 router。
- `backend/tests/inventory_audit.rs`（新）：整合測試 8 個。
- `backend/tests/fixtures/`（新）：`make_fixtures.py`（stdlib 產生器）＋ `property.xlsx`／`property.xls`（LibreOffice 轉出）／`items.xlsx`／`header_only.xlsx`／`no_header.xlsx`／`many_rows.xlsx`（5,001 列）／`max_rows.xlsx`（5,000 列）；全為合成資料。

**關鍵決定／假設**

- 顯示欄位型別：清冊側 `property_no` 為字串；`name`／`form`／`location`／`custodian`／`note` 與系統側 `note` 為可空字串——空白儲存格或缺欄＝`null`（沿用 `optional_text` 慣例、對齊 spec §5.2 範例 `note: null`）。非空值一律保留原文（不 trim、不折疊）；只有比對鍵正規化。
- 資料列判定＝編號欄含 ASCII 數字（`0-9`）；全形數字不算（實務編號皆 ASCII）。略過列＝編號欄無 ASCII 數字且財產名稱（trim 後）非空；財名稱缺欄不計略過。
- 表頭掃描以工作表「絕對列」前 10 列為準；表頭上方文字檢查同工作表所有列。
- `summary.missing_in_system`／`missing_in_list`＝該差異表列數（陣列長度）；`matched` 以去重鍵計；`duplicate`＝同鍵在該側出現多列（含系統內同鍵）。
- 排序：兩差異表皆依正規化鍵升冪、穩定排序（同鍵保留檔案／列序）。
- 超過 10 檔：讀到第 11 檔即整批拒絕（不再讀內容）；`details.files` 逐檔列已收到的檔案（訊息「超過單次 10 個檔案上限」），頂層 message「一次最多上傳 10 個檔案，整批不稽核」。
- 單檔超過 5 MB：仍耗盡該欄位以繼續讀後續檔案，讓整批錯誤能逐檔回報（超限內容不保留）。
- DateTime 儲存格（無 chrono feature）：以 Excel 序號數字字串化；`DateTimeIso`／`DurationIso` 照字串；`Error` 視同空字串。
- 唯讀：僅 `SELECT`（單一查詢取編號非空白資產），不建暫存檔；記憶體解析。

**指令與結果**

- `cargo fmt --manifest-path backend/Cargo.toml` → 套用；`-- --check` → 通過。
- `cargo test --manifest-path backend/Cargo.toml` → 全綠：**425 passed、0 failed、0 ignored**（含新單元 11＋整合 8；既有測試不受影響）。
- `cargo clippy --manifest-path backend/Cargo.toml --all-targets` → 新檔案（`src/inventory_audit.rs`、`src/api/inventory_audit.rs`、`tests/inventory_audit.rs`）零警告；其餘 15 筆皆為既有檔案的既有警告。
- `.github/workflows` 無 backend CI（僅代理發佈與一鍵安裝測試）；依既有票單慣例加跑 fmt／clippy。
- 真 HTTP 煙霧測試（登入＋curl：中文檔名 `資訊組財產清冊.xls`＋`items.xlsx` 多檔）→ 200；逐檔摘要、多檔合併、原文（多行編號、前後空白、全形空白 `　x3009999　`）保留皆正確。

**已知限制**

- `skipped` 實務值 0；合成 fixture 以 1 列驗證機制。
- 鍵排序為 UTF-8 位元組序（ASCII／中文皆 codepoint 升冪）。
- calamine 於解析時載入整本工作簿；5 MB 輸入上限已約束，但高壓縮比檔案的解壓後記憶體未另設上限。
- 超過 11 檔時僅涵蓋前 11 檔之檔名於 `details.files`。
