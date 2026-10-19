# 02 — 刪除家族（資產、介面、報廢）

Status: done
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

## Comments

實作完成（2026-10-19）；commit `ab47505`（`Kea 推送：刪除家族（票 02）`，含 `backend/src/kea/push.rs`、`backend/src/api/assets.rs`、`backend/src/api/interfaces.rs`、`backend/src/api/disposed_assets.rs`；4 檔、+260／−35）。

- 變更摘要：
  - `backend/src/kea/push.rs`：
    - `PendingDeletes { reservations: Vec<Assignment> }`（欄位私有；順序＝查詢序 `id ASC`）。
    - `capture_asset_deletion`：`interfaces::list_for_asset` → 介面 id → `assignments::list_reservations_for_interfaces`（原 `api/assets.rs` 區塊原封搬入；asset 不存在＝空清單、不 404）。
    - `capture_interface_deletion`：`list_reservations_for_interfaces(&[id])`。
    - `commit_deletions`（公開，包 `HttpPush`）／`commit_deletions_with`（內部泛型 `Option<&P> where P: KeaPush + ?Sized`）：逐筆（輸入序）網段快取（`subnets::get`；不存在→`ApiError::internal("讀取網段失敗", "網段不存在")`）→未受管／無 client 跳過→`delete`；Kea 失敗記 failed、續行。語意逐字沿用舊 `push_each`＋`after_assignment_change` 的刪除分支。
    - module 測試 +4（共 9）：跨多網段逐筆刪除且順序＝輸入序（查詢序與網段 id 序刻意相反）；未受管跳過＋無 client 跳過；單筆失敗記 failed 並續行；asset 不存在＝空清單不 404。`FakePush` 增 `failing_delete`（僅指定位址的 delete 失敗）；helper 拆出 `create_asset`／`create_interface_on`（既有 5 測的斷言與文字未動）。
  - `api/assets.rs` DELETE：`capture_asset_deletion`→`assets::delete`（false→404、不 commit）→`commit_deletions`；移除列介面＋列保留區塊與 `push_reservation_deletes` 呼叫。
  - `api/interfaces.rs` DELETE：`capture_interface_deletion`→`interfaces::delete`（false→404）→`commit_deletions`；MAC 分支的 `push_reservation_updates` 暫留（票 03）。
  - `api/disposed_assets.rs` 報廢：`capture_asset_deletion`→`disposed_assets::dispose`（None→404）→`commit_deletions`；移除不再使用的 `assignments`／`interfaces`／`kea_sync` imports。
- 差異與取捨：
  - 內部版本沿用票 01 的泛型 port 型式（`async fn` trait 無 dyn 相容性，E0038）。
  - 無 client 時仍先讀網段（與舊 `push_each` 的順序逐字相同；正常資料下結果不變）；`kea_subnet_id` 缺＝未受管跳過。
  - `push_reservation_deletes` 失去唯一呼叫點；因位於 lib 的 `pub mod sync` 仍屬公開 API，無 dead_code 警告，暫留至票 04 退場。
  - 行為、訊息、回應形狀零變更；`backend/tests/kea_sync.rs` 一字未改。
- 執行過的指令與結果（全綠）：
  - `cargo test --manifest-path backend/Cargo.toml --lib kea::push --test kea_sync --test disposal --test delete_linkage` → lib 側 `kea::push` 9 passed、0 failed（整合測試因 filter 為 0）。
  - 同上三個整合目標去 filter 全跑：kea_sync 29、disposal 7、delete_linkage 6 passed、0 failed。
  - `cargo test --manifest-path backend/Cargo.toml` → 31 個 test 目標合計 475：475 passed、0 failed、6 ignored（`tests/kea_connectivity.rs` 既有需真機 Kea 者；`--lib` 217 passed 含新增 4）。
  - `cargo fmt --manifest-path backend/Cargo.toml` 後 `-- --check` → 通過；`cargo check --manifest-path backend/Cargo.toml --all-targets`（touch 後重編）→ 無警告。
