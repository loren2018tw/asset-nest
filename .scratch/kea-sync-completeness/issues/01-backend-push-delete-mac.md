# 01: 後端：刪除與 MAC 編輯即時推送

**What to build:** 資產刪除、介面刪除與介面 MAC 編輯即時推送受管網段保留；報廢改「先搬移、後推送」、資產端指派推送移至 warnings 前（見 `.scratch/kea-sync-completeness/spec.md`、`docs/adr/0027`）。

**Blocked by:** None

**Status:** ready-for-agent

- [ ] `assignments::list_reservations_for_interfaces(pool, ids)`（單一查詢、保留用途）
- [ ] `kea::sync::push_reservation_deletes`／`push_reservation_updates`（網段快取；刪除／以最新 MAC upsert）
- [ ] `api/assets.rs`：`delete_asset` 先讀保留→本地刪除→推送→200 `{ kea_sync }`
- [ ] `api/interfaces.rs`：`delete_interface` 同上；`update_interface` MAC 清空阻擋（400）與 MAC 變更推送；回應加 `kea_sync`
- [ ] `api/disposed_assets.rs`：改用共用推送，順序改先搬移後推送
- [ ] `api/asset_assignments.rs`：推送移至 warnings 前
- [ ] 整合測試（stub Kea）：刪除成功／失敗、MAC upsert、MAC 清空阻擋／允許、未受管不推送
- [ ] 既有測試更新（204→200、`{ kea_sync: [] }`）；`cargo test`、`cargo fmt --check` 全綠

## Comments
