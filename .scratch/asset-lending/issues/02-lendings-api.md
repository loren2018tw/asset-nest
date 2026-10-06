# 02 — 借還 API 與資產端點整合

Status: done
Blocked by: 01

## 目標

實作 `spec §4`：借還 REST 端點、資產清單附帶出借資訊、刪除出借中資產的阻擋，以及整合測試。

## 範圍

- 新增 `backend/src/api/lendings.rs`，並在 `backend/src/api/mod.rs` 掛載 `mod lendings;`＋`.merge(super::lendings::router())`：
  - `POST /api/v1/assets/{id}/lendings`：body `LendingInput`；資產不存在 → `ApiError::not_found`；已有未歸還 → `ApiError::conflict("此資產已在出借中")`；驗證錯誤 → `ApiError::validation`（field `borrower`／`due_at`）；201＋`Lending`。
  - `POST /api/v1/lendings/{id}/return`：不存在 → 404；已歸還 → 409「此借出紀錄已歸還」；200＋`Lending`。
  - `GET /api/v1/lendings?returned=false`：`{ items: [LendingWithAsset] }` 不分頁。
  - `GET /api/v1/lendings?returned=true&page=&per_page=`：`{ items, total, page, per_page }`；`per_page` 預設 10、夾 1..=100（比照 assets `ListQuery`）；`page` 由 1 起。
  - `GET /api/v1/lendings/borrowers`：`{ items: [String] }`。
- `backend/src/api/assets.rs`：
  - `AssetListRow` 新增 `lending: Option<LendingBrief>`（`LendingBrief { id, borrower, lent_at, due_at }`，Serialise 出 camelCase 由前端對映）；list 查詢以 `LEFT JOIN lendings ON lendings.asset_id = assets.id AND lendings.returned_at IS NULL` 載入，避免 N+1（比照既有分組方式，注意每資產至多一列出借中）。
  - `delete_asset`：刪除前 `lendings::has_open_lending` → 409「此資產出借中，請先歸還」。
- 整合測試 `backend/tests/lendings.rs`：
  - 建立借出 → 出借中清單出現且含資產欄位與 overdue；歸還 → 移入已歸還清單、`returned_at` 非空。
  - 同一資產重複借出 → 409；歸還已歸還 → 409。
  - 驗證錯誤：borrower 空白 → 400（field `borrower`）。
  - 分頁：`page`／`per_page`／`total` 正確、越界頁回空 items。
  - `borrowers`：多筆後 distinct、最近在前。
- 既有測試補強：
  - `backend/tests/assets.rs`：list 回應含 `lending`（未出借為 null；借出後帶 borrower）。
  - `backend/tests/delete_linkage.rs`：刪除資產連動刪除 lendings（含已歸還歷史）；出借中刪除 → 409、歸還後可刪。

## 驗收

- `cargo test --manifest-path backend/Cargo.toml --test lendings --test assets --test delete_linkage`
- `cargo fmt --manifest-path backend/Cargo.toml -- --check`
- 不動前端；既有測試全綠。

## 注意

- 錯誤訊息文案以 spec §4 為準；不要 `git commit`；不要動 `.scratch/` 內其他票。

## Comments

- 2026-10-07（票 02 完成）：新增 `backend/src/api/lendings.rs`（POST `/assets/{id}/lendings`、POST `/lendings/{id}/return`、GET `/lendings?returned=false|true`、GET `/lendings/borrowers`）並掛載於 `api/mod.rs`；`assets.rs` 的 `AssetListRow` 新增 `lending: Option<LendingBrief>`、`delete_asset` 阻擋出借中刪除（409）。`LendingBrief` 票 01 未定義，補於 `backend/src/lendings.rs`（含批次查詢 `open_briefs_for_assets`）。整合測試：新增 `tests/lendings.rs` 5 例；`tests/assets.rs`、`tests/delete_linkage.rs` 各補 1 例。
  - 驗收：`cargo test --test lendings --test assets --test delete_linkage` 全綠（34 passed）；`cargo test` 後端全綠；`cargo fmt --check` 通過。
  - 偏離票文說明：(1) 「Serialise 出 camelCase」與 spec §4 欄位名及全站 snake_case 慣例（`property_no`、`lent_at` 等）衝突，採 snake_case；(2) `lending` 摘要以單一批次查詢（LEFT JOIN＋`IN` 分組）載入，比照既有 `last_seen_for_assets` 慣例，未改動 assets 主查詢；(3) `GET /lendings` 未帶 `returned` 視為 `false`（出借中），spec 未規範此情形；(4) `LendingInput` JSON 內未收的未知欄位採 serde 預設忽略。
