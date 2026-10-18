# 04 — 舊 API 與死碼退場、文件與全量回歸

Status: ready-for-agent
Blocked by: 03

## 目標

刪除被取代的舊推送 API 與 0 引用的死抽象；更新文件；全量回歸確認沒有任何殘留使用者。

## 範圍

- `backend/src/kea/sync.rs`：刪除 `KeaSync`（已移入 push）、`after_assignment_change`、`push_reservation_deletes`、`push_reservation_updates`、`push_each`、私有 `upsert`、`KeaSync::ok/failed`；更新模組 `//!` 文件（單筆推送指向 [`push`]；完整同步描述不變）。確認完整同步（計畫／套用）行為與測試完全不動。
- `backend/src/kea/mod.rs`：刪除 `KeaGateway`、`Reservation`、`Lease`、`KeaError::NotImplemented`（確認全 repo 0 引用）；更新模組 `//!`（移除「以 KeaGateway 定義，待後續階段實作」句）。
- 其他：以 `cargo check --all-targets` 與 `grep` 確認無殘留（`KeaGateway`、`push_reservation_`、`after_assignment_change`、`kea::sync::KeaSync`）。
- 確認 `KeaError` 剩餘 variant（`Http`／`Response`／`Malformed`／`Data`）都有使用者。

## 驗收

- `grep -rn "KeaGateway\|after_assignment_change\|push_reservation_deletes\|push_reservation_updates\|push_each" backend/src` 無命中
- `cargo test --manifest-path backend/Cargo.toml` 全綠（回報總數；kea_sync 29 測不變）
- `cargo fmt --manifest-path backend/Cargo.toml` 後 `-- --check` 綠；`cargo check --all-targets` 無警告

## 交付

1. commit：`Kea 推送：舊 API 與死碼退場（票 04）`。
2. 本票檔：`Status: done`、`## Comments` 補 commit hash 與摘要。
3. commit：`Kea 推送：票 04 補記實作與 commit hash`。
