# 02 — 刪除家族（資產、介面、報廢）

Status: ready-for-agent
Blocked by: 01

## 目標

`capture_asset_deletion`／`capture_interface_deletion`／`commit_deletions`＋module 測試；遷移三個刪除型 handler。

## 範圍

- `backend/src/kea/push.rs`：
  - `pub struct PendingDeletes { reservations: Vec<Assignment> }`（欄位不公開；順序＝查詢序）。
  - `pub async fn capture_asset_deletion(db, asset_id) -> Result<PendingDeletes, ApiError>`：`interfaces::list_for_asset` → 介面 id → `assignments::list_reservations_for_interfaces`（現行 `api/assets.rs` 的區塊原封搬入；asset 不存在＝空清單，不 404）。
  - `pub async fn capture_interface_deletion(db, interface_id) -> Result<PendingDeletes, ApiError>`：`list_reservations_for_interfaces(&[id])`。
  - `pub async fn commit_deletions(db, kea: Option<&Client>, pending) -> Result<Vec<KeaSync>, ApiError>`：逐筆（順序不變）以網段快取（`subnets::get`，不存在→`ApiError::internal("讀取網段失敗", "網段不存在")`）→ 未受管或無 client 跳過 → `delete`；失敗記 failed、續行。internal 版本供測試注入 port。
  - 測試：spec §測試決策「刪除」子清單。
- 遷移（保留既有 404／出借中判斷與順序）：
  - `api/assets.rs` DELETE：`capture_asset_deletion` → `assets::delete`（false→404、不 commit）→ `commit_deletions`。
  - `api/interfaces.rs` DELETE：`capture_interface_deletion` → `interfaces::delete`（false→404）→ `commit_deletions`。
  - `api/disposed_assets.rs`：`capture_asset_deletion` → `disposed_assets::dispose`（None→404）→ `commit_deletions`。
- 舊 `push_reservation_deletes`／`push_each` 暫留（`api/interfaces.rs` 的 MAC 分支仍用 `push_reservation_updates`，票 03 遷移；票 04 統一退場）。

## 驗收

- `cargo test --manifest-path backend/Cargo.toml --lib kea::push --test kea_sync --test disposal --test delete_linkage`
- `cargo test --manifest-path backend/Cargo.toml` 全綠（29 個 kea_sync 測斷言不得改動）
- `cargo fmt --manifest-path backend/Cargo.toml` 後 `-- --check` 綠；`cargo check --all-targets` 無新警告

## 交付

1. commit：`Kea 推送：刪除家族（票 02）`。
2. 本票檔：`Status: done`、`## Comments` 補 commit hash 與摘要。
3. commit：`Kea 推送：票 02 補記實作與 commit hash`。
