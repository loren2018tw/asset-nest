# 01 — push module、port 與指派家族

Status: ready-for-agent
Blocked by: —

## 目標

依 spec §介面／§觸發規則，新增 `backend/src/kea/push.rs`：`KeaSync`（自 `sync.rs` 移入）、內部 `KeaPush` port 與 `HttpPush` adapter、`PendingAssignment`、`capture_assignment`／`commit_assignment`，以及 module 測試。遷移指派家族的三個呼叫點。

## 範圍

- `backend/src/kea/push.rs`（新）：
  - `pub struct KeaSync`（`Serialize` 同現行 `{status, message}`；`ok()`／`failed()`）。
  - `trait KeaPush { upsert; delete }`（私有）＋`struct HttpPush<'a>(&'a Client)`：`upsert`＝del（忽略 bool）→add→config-write；`delete`＝del，僅實際刪到才 config-write（語意逐字沿用現行 `upsert`／`after_assignment_change` 的 delete 分支）。
  - `pub struct PendingAssignment { subnet: Option<Subnet>, address: String, before: Option<Assignment> }`（欄位不公開）。
  - `pub async fn capture_assignment(db, address: IpAddr) -> Result<PendingAssignment, ApiError>`：`subnets::find_by_address`；找不到→`subnet=None`（永不推送）；找到→`assignments::find(subnet.id, address)`（DB 錯誤→`ApiError::internal("讀取指派失敗", …)`）。
  - `pub async fn commit_assignment(db, kea: Option<&Client>, pending) -> Result<Option<KeaSync>, ApiError>`：依 spec 觸發規則（重讀 after；upsert 需介面 MAC，缺 MAC→`KeaError::Data("保留位址 {a} 的介面缺少 MAC")`→failed；delete 取 before，缺 before→`ApiError::internal("Kea 同步失敗", "缺少變更前的指派狀態")`）。
  - internal 版本（供測試注入 port）：`commit_assignment_with(db, kea: Option<&dyn KeaPush>, pending)`；公開函式包 `Option<&Client>`。
  - 測試：spec §測試決策「指派」子清單（in-memory SQLite＋migrations＋`FakePush`）。
- `backend/src/kea/mod.rs`：`pub mod push;`。
- `backend/src/kea/sync.rs`：`KeaSync` 定義移除，暫以 `use super::push::KeaSync;` 供未遷移的舊函式使用。
- 匯入更新（`crate::kea::sync::KeaSync` → `crate::kea::push::KeaSync`）：`api/ips.rs`、`api/asset_assignments.rs`、`api/interfaces.rs`、`api/assets.rs`、`api/disposed_assets.rs`（本票僅換 import；後三者的呼叫遷移在票 02／03）。
- 遷移：
  - `api/ips.rs` PUT／DELETE：`capture_assignment`（在 `assign`／`cancel` 之前，用路徑位址）→ 本地異動 → `commit_assignment`。移除本檔的 before 查詢與 `after_assignment_change` 呼叫。
  - `api/asset_assignments.rs`：`capture_assignment`（在 `assign_for_asset` 之前）→ 本地異動 → `commit_assignment`；移除 `subnets::find_by_address`＋`assignments::find` 的 before 區塊（其餘查詢照舊）。
- 舊 `after_assignment_change` 暫留（票 04 退場），供未遷移的 `push_each` 使用。

## 驗收

- `cargo test --manifest-path backend/Cargo.toml --lib kea::push --test kea_sync --test assignments --test asset_assignments --test ips`
- `cargo test --manifest-path backend/Cargo.toml` 全綠（回報總數；29 個 kea_sync 測不得改動斷言）
- `cargo fmt --manifest-path backend/Cargo.toml` 後 `-- --check` 綠；`cargo check --all-targets` 無新警告

## 交付

1. commit：`Kea 推送：push module 與指派家族（票 01）`。
2. 本票檔：`Status: done`、`## Comments` 補 commit hash 與摘要。
3. commit：`Kea 推送：票 01 補記實作與 commit hash`。
