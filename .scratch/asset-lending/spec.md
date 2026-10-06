# spec — 資產借還（asset lending）

- 狀態：已定案（經 2026-10-07 grilling 兩輪逐題確認，全部採納建議）
- 詞彙：`GLOSSARY.md`「借還詞彙」（借出、出借中、歸還、借出紀錄 Lending、借用人、預計歸還日）已登錄

## 1. 背景與目標

資產目前只有 CRUD 與 IP 指派，無「借出給人」的概念。實務上設備（如筆電）會暫時借給同仁使用，需要：

1. 資產清單操作欄新增「借出」按鈕（每列），記錄借用人、預計歸還日（選填）、備註（選填）。
2. 新增「資產借還」頁（`/lendings`）：上方「出借中」列表＋快速歸還按鈕；下方「已歸還紀錄」分頁列表。
3. 出借中的資產不可再次借出、不可刪除（仍可編輯）；資產刪除時其借還紀錄一併刪除。

## 2. 名詞與不變量

- **借出紀錄（Lending）**：一筆「借出到歸還」的完整紀錄。欄位：所屬 Asset、借用人、借出時間、歸還時間，及選填的預計歸還日與備註。
- **出借中**：Asset 存在一筆未歸還借出紀錄的狀態。由未歸還紀錄推導，**不新增 Asset 狀態欄位**。
- **逾期**：預計歸還日已過且尚未歸還；由後端判定（比照 assets 屆齡的 today 處理）。

不變量：

- 同一 Asset 同時**至多一筆**未歸還借出紀錄；建立第二筆 → 409。
- 已歸還的紀錄不可再次歸還 → 409。
- 出借中的 Asset 不可刪除 → 409「此資產出借中，請先歸還」。
- 資產刪除時其全部借還紀錄（含已歸還歷史）連動刪除（FK CASCADE）。
- 借出時間一律記伺服器當下（UTC ISO8601），不開放修改。
- 借出紀錄不可編輯（v1 無 PATCH）；打錯＝歸還→重新借出。

## 3. 資料模型

新增 `backend/migrations/0011_lendings.sql`：

```sql
CREATE TABLE IF NOT EXISTS lendings (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    asset_id    INTEGER NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
    borrower    TEXT    NOT NULL,
    lent_at     TEXT    NOT NULL,
    due_at      TEXT,
    note        TEXT,
    returned_at TEXT
);
CREATE INDEX IF NOT EXISTS idx_lendings_asset_id ON lendings(asset_id);
CREATE INDEX IF NOT EXISTS idx_lendings_returned ON lendings(returned_at);
```

- 時間格式與 assets 一致：`lent_at`／`returned_at` 為 UTC ISO8601 TEXT（`strftime('%Y-%m-%dT%H:%M:%SZ','now')`），由伺服器產生；`due_at` 為 `YYYY-MM-DD` 日期字串（同 `purchase_date`）。
- 新模組 `backend/src/lendings.rs`：
  - 列／API 型別：`LendingRow`(FromRow)、`pub struct Lending`（含 `asset_id`、`borrower`、`lent_at`、`due_at: Option<String>`、`note: Option<String>`、`returned_at: Option<String>`、`overdue: bool`）、`LendingWithAsset`（在 `Lending` 上附加 `property_no`、`description`，供列表呈現）、`LendingInput { borrower: Option<String>, due_at: Option<String>, note: Option<String> }`、`ValidLending`。
  - 驗證：`borrower` trim 後非空（必填）；`due_at` 若提供須為 `YYYY-MM-DD` 格式；`note` 自由文字（可空）。
  - 查詢：`has_open_lending(asset_id)`、`list_open()`（`returned_at IS NULL`，依 `lent_at` 倒序）、`list_returned(page, per_page)`（`returned_at IS NOT NULL`，依 `lent_at` 倒序，回 `{ items, total, page, per_page }`）、`list_borrowers()`（distinct 非空 borrower，依最近借出時間倒序）、`create(asset_id, ValidLending)`、`return_one(id)`。
  - `overdue`：僅對未歸還且 `due_at` 存在者判定——`due_at < 今天` 為 true（比照 `assets::is_expired` 的 today 注入方式）。

## 4. API

新模組 `backend/src/api/lendings.rs`，掛載於 `api/mod.rs`：

| 方法 | 路徑 | 行為 |
|------|------|------|
| POST | `/api/v1/assets/{id}/lendings` | 建立借出。body `{ borrower, due_at?, note? }`。資產不存在 → 404；已有未歸還借出 → 409「此資產已在出借中」；驗證錯誤 → 400（field `borrower`／`due_at`）。回 201＋`Lending` |
| POST | `/api/v1/lendings/{id}/return` | 歸還。記 `returned_at = 當下`。不存在 → 404；已歸還 → 409「此借出紀錄已歸還」。回 200＋`Lending` |
| GET | `/api/v1/lendings?returned=false` | 出借中清單：`{ items: [LendingWithAsset] }`，不分頁，`lent_at` 倒序 |
| GET | `/api/v1/lendings?returned=true&page=&per_page=` | 已歸還紀錄：分頁 `{ items: [LendingWithAsset], total, page, per_page }`，預設每頁 10、`lent_at` 倒序；`page` 由 1 起 |
| GET | `/api/v1/lendings/borrowers` | 借用人建議：`{ items: [String] }`，最近使用者在前 |

資產既有端點整合：

- `GET /api/v1/assets`（list）：`AssetListRow` 新增 `lending: Option<LendingBrief>`，`LendingBrief { id, borrower, lent_at, due_at }`；以 `LEFT JOIN`（`returned_at IS NULL`）載入，`null` 表未出借。避免 N+1（比照既有清單查詢分組方式）。
- `DELETE /api/v1/assets/{id}`：刪除前檢查未歸還借出，有 → 409「此資產出借中，請先歸還」；無 → 既有刪除流程（借還紀錄由 FK CASCADE 連動刪除）。

## 5. 前端：資產清單（`AssetsPage.vue`）

- 操作欄最左新增借出按鈕，依該列 `lending` 欄位切換：
  - 未出借：`q-btn flat dense round icon="person_add" aria-label="借出"` → 開 `LendingDialog`。
  - 出借中：改為 `q-btn flat no-caps label="出借中" color="orange" disable`，tooltip「出借中：{借用人}」。不加欄位、不加 chip。
  - 出借中列的刪除按鈕停用，tooltip「出借中，請先歸還」（前端預擋；後端 409 為權威）。編輯按鈕維持可用。
- `AssetListRow` 型別（`frontend/src/api/assets.ts`）新增 `lending?: LendingBrief | null`。
- 新元件 `frontend/src/components/LendingDialog.vue`：
  - 標題顯示資產資訊（property_no＋描述）以確認對象。
  - 欄位：借用人（`q-select` `use-input`，選項來自 `GET /lendings/borrowers`，仍可自由輸入；必填）、預計歸還日（`q-input type="date"`；選填）、備註（`q-input type="textarea"`；選填）。
  - 送出 → `POST /assets/{id}/lendings` → `$q.notify` 成功 → 關閉並重取清單。
- 新 API 模組 `frontend/src/api/lendings.ts`（`createLending`、`returnLending`、`listOpenLendings`、`listReturnedLendings`、`listBorrowers`）。

## 6. 前端：資產借還頁（`LendingsPage.vue`）

- 路由：`frontend/src/router/routes.ts` 新增 `{ path: "lendings", component: ... }`；側欄「資產管理」區段（資產清單之下）新增入口「資產借還」。
- 頁面標題「資產借還」，上下兩區塊：

### 6.1 出借中（上方）

- 不分頁，一次全列（`GET /lendings?returned=false`）。
- 欄位：資產（property_no＋描述）、借用人、借出時間、預計歸還日、操作。
- 「快速歸還」：`q-btn` 一鍵呼叫 `POST /lendings/{id}/return`，**不彈確認框、不問欄位**；成功 `$q.notify`「已歸還」並重取上下兩區塊。按鈕呼叫期間以 loading 狀態防連點。
- 逾期：該筆 `overdue === true` 時，預計歸還日旁顯示紅色「逾期」標籤。
- 空狀態：「目前沒有出借中的資產」。
- 時間顯示：`toLocaleString("zh-TW", { hour12: false })`（比照 KeaLeasesPage）；日期原樣顯示。

### 6.2 已歸還紀錄（下方）

- 伺服器端分頁表格（仿 AssetsPage：`v-model:pagination`＋`@request`；預設每頁 10、`lent_at` 倒序，無排序選項、無搜尋）。
- 欄位：資產（property_no＋描述）、借用人、借出時間、歸還時間、預計歸還日。
- 紀錄不可刪除（append-only）。

## 7. 非目標（v1）

- 編輯／刪除借出紀錄；批次借出（一次借多台）。
- 借還頁搜尋／篩選；資產清單「出借中」篩選或狀態欄位。
- 資產匯出 CSV（14 欄範本，ADR-0008）納入借出資訊。
- 逾期通知／提醒；歸還備註或設備狀況欄位。
- 借用人獨立實體（自由文字，同 location 先例）。

## 8. 測試矩陣

| 模組 | 測試重點 |
|------|----------|
| migration／delete_linkage | 刪除資產連動刪除 `lendings`（含已歸還歷史） |
| lendings 單元 | borrower trim 非空、due_at 格式、overdue 判定（含無 due_at、當天不逾期）、分頁參數 |
| tests/lendings | 建立→歸還往返；重複借出 409；重複歸還 409；`returned=false/true` 清單；borrowers 建議排序；分頁欄位 |
| tests/assets | list 帶 `lending` 欄位（出借中／未出借）；刪除出借中資產 409；歸還後可刪除 |
| frontend | `pnpm --filter frontend typecheck`＋`pnpm lint:check`；人工檢核清單見各票 |

## 9. 實作票

| # | 票 | 依賴 |
|---|----|------|
| 01 | lendings 資料模型與核心邏輯 | — |
| 02 | 借還 API 與資產端點整合 | 01 |
| 03 | 前端：資產清單借出按鈕與對話框 | 02 |
| 04 | 前端：資產借還頁、路由與側欄 | 02 |
| 05 | 整合驗證 | 全部 |
