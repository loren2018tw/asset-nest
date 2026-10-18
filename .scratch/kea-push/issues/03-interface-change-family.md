# 03 — MAC 編輯家族

Status: ready-for-agent
Blocked by: 02

## 目標

`capture_interface_change`／`commit_interface_change`＋module 測試；遷移 `api/interfaces.rs` 更新路徑，移除 route 的 MAC 比較與保留查詢。

## 範圍

- `backend/src/kea/push.rs`：
  - `pub struct PendingInterfaceChange { interface_id: i64, mac_before: Option<String>, reservations: Vec<Assignment> }`（欄位不公開）。
  - `pub async fn capture_interface_change(db, interface_id) -> Result<PendingInterfaceChange, ApiError>`：`interfaces::get`（不存在→`mac_before=None`、保留空）＋`list_reservations_for_interfaces(&[id])`。
  - `pub async fn commit_interface_change(db, kea: Option<&Client>, pending) -> Result<Option<Vec<KeaSync>>, ApiError>`：重讀介面（不存在→`None`）；MAC 未變→`None`；變更→對保留逐筆以新 MAC upsert（網段快取；未受管跳過；介面無 MAC→`KeaError::Data("保留位址 {a} 的介面缺少 MAC")` 記 failed）；空結果→`None`。internal 版本供測試注入 port。
  - 測試：spec §測試決策「MAC」子清單。
- `api/interfaces.rs` PATCH：`capture_interface_change`（在 `interfaces::update` 之前）→ update → `commit_interface_change`；刪除 `if existing.mac != interface.mac` 區塊與 `kea_sync::push_reservation_updates` 呼叫（`existing` 仍供 patch 驗證與清空 MAC 結構規則使用）。
- 舊 `push_reservation_updates` 至此無呼叫者（`pub` 不觸發警告；票 04 退場）。

## 驗收

- `cargo test --manifest-path backend/Cargo.toml --lib kea::push --test kea_sync --test interfaces`
- `cargo test --manifest-path backend/Cargo.toml` 全綠（29 個 kea_sync 測斷言不得改動）
- `cargo fmt --manifest-path backend/Cargo.toml` 後 `-- --check` 綠；`cargo check --all-targets` 無新警告

## 交付

1. commit：`Kea 推送：MAC 編輯家族（票 03）`。
2. 本票檔：`Status: done`、`## Comments` 補 commit hash 與摘要。
3. commit：`Kea 推送：票 03 補記實作與 commit hash`。
