# 01: 後端：刪除與 MAC 編輯即時推送

**What to build:** 資產刪除、介面刪除與介面 MAC 編輯即時推送受管網段保留；報廢改「先搬移、後推送」、資產端指派推送移至 warnings 前（見 `.scratch/kea-sync-completeness/spec.md`、`docs/adr/0027`）。

**Blocked by:** None

**Status:** done

- [x] `assignments::list_reservations_for_interfaces(pool, ids)`（單一查詢、保留用途）
- [x] `kea::sync::push_reservation_deletes`／`push_reservation_updates`（網段快取；刪除／以最新 MAC upsert）
- [x] `api/assets.rs`：`delete_asset` 先讀保留→本地刪除→推送→200 `{ kea_sync }`
- [x] `api/interfaces.rs`：`delete_interface` 同上；`update_interface` MAC 清空阻擋（400）與 MAC 變更推送；回應加 `kea_sync`
- [x] `api/disposed_assets.rs`：改用共用推送，順序改先搬移後推送
- [x] `api/asset_assignments.rs`：推送移至 warnings 前
- [x] 整合測試（stub Kea）：刪除成功／失敗、MAC upsert、MAC 清空阻擋／允許、未受管不推送
- [x] 既有測試更新（204→200、`{ kea_sync: [] }`）；`cargo test`、`cargo fmt --check` 全綠

## Comments

實作完成（主實作 commit `393bcda`；決策見 `docs/adr/0027`）。

- `kea::sync` 新增 `push_reservation_deletes`／`push_reservation_updates`（共用 `push_each`，網段以 `subnet_id` 快取；未受管網段或未設 client 不產生結果）。
- `kea::sync` 兩函式名經剩餘重複清理（`push_reservation_refreshes` 舊名移除），`api/interfaces.rs` 以 `push_reservation_updates` 呼叫。
- `api/assets.rs` `delete_asset` 與 `api/interfaces.rs` `delete_interface`：先以 `assignments::list_reservations_for_interfaces` 擷取保留，本地刪除後推送，回應 200＋`kea_sync: Vec<KeaSync>`（未設 Kea 為空陣列）。
- `api/interfaces.rs` `update_interface`：`existing.mac.is_some() && valid.mac.is_none()` 且有保留指派 → 400 `validation_error`（field `mac`，訊息含筆數）；MAC 有變更時推送更新，`kea_sync` 僅在非空時出現（`skip_serializing_if`）。
- `api/disposed_assets.rs`：移除原「推前搬後」私有函式，改共用推送、順序「擷取 → 搬移 → 推送」。
- `api/asset_assignments.rs`：`kea_sync::after_assignment_change` 移至 `warnings_for_address` 之前。
- `interfaces.rs` `ValidInterface` 欄位改 `pub(crate)`（供 API 層檢查 MAC 變更）。
- 測試：`tests/kea_sync.rs` 新增 6 個（資產／介面刪除推送、刪除失敗仍本地、MAC upsert、MAC 清空阻擋／允許、未涉同步不附 `kea_sync`）；`tests/assets.rs`、`tests/assignments.rs`、`tests/delete_linkage.rs`、`tests/interfaces.rs` 更新為 200＋`kea_sync` 斷言。`cargo test`（31 組、含 195 單元）全綠。
