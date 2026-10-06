# 01 — lendings 資料模型與核心邏輯

Status: done
Blocked by: —

## 目標

實作 `spec §3`：migration、`backend/src/lendings.rs` 型別、驗證與查詢。此票只做資料層與領域邏輯，不動 API handler、資產端點與前端（後續票）。

## 範圍

- 新增 `backend/migrations/0011_lendings.sql`（schema 見 spec §3；含 `idx_lendings_asset_id`、`idx_lendings_returned`）。
- 新增 `backend/src/lendings.rs`（比照 `assets.rs` 的風格與測試方式）：
  - `LendingRow`(FromRow)、`pub struct Lending`（`id, asset_id, borrower, lent_at, due_at: Option<String>, note: Option<String>, returned_at: Option<String>, overdue: bool`）、`LendingWithAsset`（附加 `property_no`、`description`）、`LendingInput`、`ValidLending`。
  - `validate_fields`：borrower trim 後非空；due_at 若提供須為 `YYYY-MM-DD` 格式（回報欄位 `borrower`／`due_at`）。
  - 查詢：`has_open_lending`、`list_open`（`returned_at IS NULL`，`lent_at` 倒序）、`list_returned(page, per_page)`（`returned_at IS NOT NULL`，`lent_at` 倒序，回 total）、`list_borrowers`（distinct 非空，依最近借出倒序）、`create`（`lent_at` 以 `strftime('%Y-%m-%dT%H:%M:%SZ','now')` 產生）、`return_one`（更新 `returned_at`，回傳更新後列）。
  - `overdue` 判定：未歸還且 `due_at` 存在且 `due_at < 今天`；today 以參數注入（比照 `assets::is_expired`），UTC ISO8601 時間欄位不參與判定。
  - 查詢時間欄位與 `LendingRow`／`Lending` 對映：`due_at`／`returned_at` 可能為 NULL。
- 單元測試（`lendings.rs` 內 `#[cfg(test)]`，用 sqlx `SqlitePool::connect(":memory:")` 或既有測試 helper 模式）：
  - 驗證：borrower 空白／trim 後空白 → 錯；due_at 格式錯（`2026/1/1`、`1-2-3`）→ 錯；合法輸入 → 過。
  - overdue：無 due_at → false；due_at 為昨天 → true；今天 → false；已歸還 → false。
  - 建立後 `has_open_lending` 為 true；`return_one` 後為 false。
- 若 `main.rs`／`lib.rs` 需 `mod lendings;` 先加（此票不掛 API 路由）。

## 驗收

- `cargo test --manifest-path backend/Cargo.toml --lib lendings`
- `cargo fmt --manifest-path backend/Cargo.toml -- --check`
- 不動 API、不動前端；既有測試全綠。

## 注意

- 風格比照現有模組：中文註解、snake_case、`Option<String>` 欄位。
- 不要 `git commit`；不要動 `.scratch/` 內其他票。

## Comments

### 2026-10-07 完成紀錄（agent）

- 新增 `backend/migrations/0011_lendings.sql`：schema 照 spec §3（含 `idx_lendings_asset_id`、`idx_lendings_returned`），繁體中文註解。
- 新增 `backend/src/lendings.rs`：
  - 型別：`LendingRow`、`Lending`、`LendingWithAsset`（flatten `Lending`＋`property_no`/`description`）、`LendingInput`、`ValidLending`、`ReturnOutcome`。
  - 驗證：borrower trim 後非空（field `borrower`）、due_at 嚴格 `YYYY-MM-DD`（4-2-2 位數字＋`NaiveDate` 日曆檢查，field `due_at`；chrono 對 `1-2-3` 寬容，故加位數檢查以符合票的範例）、note 自由文字 trim。
  - 查詢：`has_open_lending`、`list_open`、`list_returned(page, per_page)`、`list_borrowers`（視窗函式 NOCASE 去重、依最近 `lent_at` 倒序）、`create`（`lent_at` 由 `strftime('%Y-%m-%dT%H:%M:%SZ','now')` 產生）、`return_one`（`WHERE returned_at IS NULL` 原子守門）。
  - overdue：未歸還且 due_at 存在且 `< today`；today 以參數注入（比照 `assets::is_expired`）。
- `backend/src/lib.rs` 加 `pub mod lendings;`（kea 與 observation 之間，字母序）。
- 單元測試 9 支全綠：驗證（borrower 空白/trim、due_at 格式含 `2026/1/1`、`1-2-3`）、overdue（無 due_at／昨天／今天／已歸還）、建立→歸還往返（has_open 轉換、重複歸還 `AlreadyReturned`、不存在 `NotFound`）、分頁與倒序、borrowers 建議（去重、排除空白、最近在前）。
- 驗收：`cargo test --manifest-path backend/Cargo.toml --lib lendings` → 9 passed；`cargo fmt --manifest-path backend/Cargo.toml -- --check` → 通過；`cargo test --lib` 全量 151 passed（既有測試全綠）。

偏離說明（無規格衝突）：

- `list_returned` 回傳 `(Vec<LendingWithAsset>, i64)`（items＋total），而非 `{items, total, page, per_page}` 結構——票 01 僅要求「回 total」，分頁包裝屬 API 層（票 02）職責，與 `assets::list` 一致。
- `return_one` 回傳 `ReturnOutcome` 列舉（`Returned`／`NotFound`／`AlreadyReturned`），供票 02 直接對映 404／409，而非僅 `Option<Lending>`。
