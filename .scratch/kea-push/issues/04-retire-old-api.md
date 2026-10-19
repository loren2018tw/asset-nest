# 04 — 舊 API 與死碼退場、文件與全量回歸

Status: done
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

## Comments

實作完成（2026-10-19）；commit `1b32cae`（`Kea 推送：舊 API 與死碼退場（票 04）`，含 `backend/src/kea/sync.rs`、`backend/src/kea/mod.rs`、`backend/src/kea/push.rs`；3 檔、+9／−192）。

- 變更摘要：
  - `backend/src/kea/sync.rs`：刪除 `after_assignment_change`、`push_reservation_deletes`、`push_reservation_updates`、`push_each`、私有 `upsert`；移除 `use super::push::KeaSync`、`use crate::interfaces`、`use crate::subnets::Subnet`，`use crate::assignments::{self, Assignment}` 收斂為 `use crate::assignments`（`Assignment` 僅被刪除函式使用；`Subnet` 由 `interfaces` 移除後才失效）。`//!` 首行改「Kea 完整同步（見 `docs/adr/0011`）」，單筆推送段落改指 [`push`](super::push)（ADR-0027）；完整同步描述、`upsert` 語意句與整段計畫／套用實作逐字未動。
  - `backend/src/kea/mod.rs`：刪除 `KeaGateway`（含 `#[allow(async_fn_in_trait)]`）、`Reservation`、`Lease`、`KeaError::NotImplemented` 與隨之無用的 `Ipv4Addr`／`Ipv4Net` import；`//!` 移除「以 [`KeaGateway`] 定義，待後續階段實作」句，改為 `[`push`]`（ADR-0027）／`[`sync`]`（ADR-0011）分指。
  - `backend/src/kea/push.rs`（僅文件，見下「差異與取捨」）：`//!` 移除「逐字沿用舊 `kea::sync::after_assignment_change`」括註；`KeaSync::ok` 文件移除「供 [`super::sync`] 舊推送函式共用（票 04 前暫留）」半句。
- 驗證：
  - `grep -rn "KeaGateway\|after_assignment_change\|push_reservation_deletes\|push_reservation_updates\|push_each" backend/src` → 無命中（exit 1）。改動前唯一殘留為 `push.rs` 兩處文件（見下）；`backend/tests` 無此類引用。
  - `KeaError` 剩餘 variant 均有使用者：`Response`（`http.rs` 回應錯誤、`tests/kea_connectivity.rs` 比對）、`Malformed`（`http.rs` 解析五處）、`Data`（`push.rs` 缺 MAC／`sync.rs` 缺原始網段設定）、`Http`（`#[from] reqwest::Error`；`http.rs` `request.send().await?`／`response.text().await?` 轉換）。
  - `cargo test --manifest-path backend/Cargo.toml` → 31 個 test 目標合計 485：479 passed、0 failed、6 ignored（`tests/kea_connectivity.rs` 既有需真機 Kea 者）；`--lib` 221 passed（含 `kea::push` 13）、`tests/kea_sync.rs` 29 一字未改且全綠。
  - `cargo fmt --manifest-path backend/Cargo.toml` 後 `-- --check` → 通過；`cargo check --manifest-path backend/Cargo.toml --all-targets`（touch `backend/src/**` 後強制重編）→ 無警告（首輪 `Subnet` unused import 已修掉）。
- 差異與取捨：
  - **唯一超出票列檔案的改動＝`push.rs` 兩處註解**（零行為）。驗收要求 `backend/src` 對 `after_assignment_change` 等字串無命中，而 `push.rs` 檔頭的「逐字沿用舊 `kea::sync::after_assignment_change`」與 `KeaSync::ok` 的「供 `super::sync` 舊推送函式共用（票 04 前暫留）」在退場後即成失效敘述；以刪字最小修正並於此列明。
  - `KeaSync::ok`／`failed` 維持 `pub(crate)` 未動（`crate` 內已無其他使用者，非 `pub` 不觸發 dead_code 警告）；`pub mod sync` 對外型別集合亦未動，無其他檔案需配合修改、無編譯錯誤。
  - `backend/src` 之外的 `KeaGateway` 字樣僅存於 `docs/adr/0010`（歷史決策紀錄，未動）與 `.scratch/` 舊票；`frontend` 的 `KeaGatewayPlan` 為網段層計畫型別、同名巧合。
  - 完整同步（計畫／套用）行為、訊息與回應形狀零變更；前端、`agent/`、spec 未動。
