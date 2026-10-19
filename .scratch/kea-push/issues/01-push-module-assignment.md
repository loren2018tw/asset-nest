# 01 — push module、port 與指派家族

Status: done
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

## Comments

實作完成（2026-10-19）；commit `8343625`（`Kea 推送：push module 與指派家族（票 01）`，含 `backend/src/kea/push.rs`（新）、`backend/src/kea/mod.rs`、`backend/src/kea/sync.rs`、`backend/src/api/ips.rs`、`backend/src/api/asset_assignments.rs`、`backend/src/api/interfaces.rs`、`backend/src/api/assets.rs`、`backend/src/api/disposed_assets.rs`；8 檔、+600／−72）。

- 變更摘要：
  - `backend/src/kea/push.rs`（新）：
    - `KeaSync` 自 `sync.rs` 移入（`Serialize`、欄位與序列化不變）；`ok()`／`failed()` 改 `pub(crate)` 供 `sync.rs` 舊函式暫用。
    - 私有 `trait KeaPush { upsert; delete }`＋`struct HttpPush<'a>(&'a Client)`：`upsert`＝`reservation_del`（忽略回傳 bool、錯誤沿用 `?`）→`reservation_add`→`config_write`；`delete`＝`reservation_del`，僅回 `true` 才 `config_write`（語意逐字沿用舊 `upsert` 與 `after_assignment_change` 的 delete 分支）。
    - `PendingAssignment`（欄位私有）、`capture_assignment`（`subnets::find_by_address`；找不到→`subnet = None`；找到→`assignments::find`；DB 錯誤→`ApiError::internal("讀取指派失敗", …)`；永不 404）。
    - `commit_assignment(db, Option<&Client>, pending)`（公開，包 `HttpPush`）／`commit_assignment_with(db, Option<&P>, pending)`（內部，port 注入）：受管網段＋client 缺一→`None`；重讀 `after`；保留→取介面 MAC（無 MAC→`KeaError::Data("保留位址 {a} 的介面缺少 MAC")`→failed）、有 MAC→upsert `ReservationRecord{ip_address, hw_address, hostname}`；`before` 保留且 `after` 非保留→delete（缺 `before`→`ApiError::internal("Kea 同步失敗", "缺少變更前的指派狀態")`）；兩態皆非保留→`None`；Kea 錯誤→`KeaSync::failed`（不 `?` 出去）。
    - module 測試 5 個（in-memory SQLite＋migrations＋`FakePush` 記命令序列、可注入失敗）：受管＋保留→恰好一次 upsert 且欄位正確；保留→static／取消→一次 delete；未受管／未設 client／static→static／static→取消→零命令＋`None`；保留缺 MAC→failed＋零命令；Kea 失敗→failed＋本地保留列仍在（不影響本地）。
  - `kea/mod.rs`：`pub mod push;`（字母序；module 文件未動，票 04 退場時更新）。
  - `kea/sync.rs`：移除 `KeaSync` 定義與 `ok`／`failed` impl，改 `use super::push::KeaSync;`；`after_assignment_change`、`push_each`、私有 `upsert` 等舊函式暫留（票 04 退場）。
  - `api/ips.rs` PUT／DELETE：`capture_assignment`（本地異動前）→`assign`／`cancel`→`commit_assignment`；移除本檔 `before` 查詢與 `after_assignment_change` 呼叫。DELETE 保留 `subnets::get` 的 404 檢查（改為不綁定結果）。
  - `api/asset_assignments.rs`：`capture_assignment`（`assign_for_asset` 前）→載網段（warnings 用）→`commit_assignment`→warnings；移除 `subnets::find_by_address`＋`assignments::find` 的 before 區塊。
  - `api/interfaces.rs`／`api/assets.rs`／`api/disposed_assets.rs`：僅把 `use crate::kea::sync::KeaSync;` 改為 `use crate::kea::push::KeaSync;`（呼叫遷移留票 02／03）。
- 差異與取捨：
  - `commit_assignment_with` 的 port 參數由票文「`Option<&dyn KeaPush>`」改為泛型「`Option<&P>` where `P: KeaPush + ?Sized`」：`async fn` trait 在穩定版 Rust 無 dyn 相容性（E0038；已實測 1.93），dyn 化需 `Pin<Box<dyn Future>>` 或 nightly；泛型為最小偏離（trait 本體逐字照 spec、無 boxing），公開函式與測試注入的用法不變，函式 doc 已註明原因。
  - `api/ips.rs` DELETE 的 `subnets::get` 由「取得 `subnet` 供 before 查詢」改為純 404 檢查；正常路徑行為相同。
  - 正常路徑與舊碼的查詢等價：capture 以路徑位址找網段（spec 決策 3），路徑網段含該位址時與舊 `subnet.id` 取 before 相同；位址不屬路徑網段時 `assign`／`cancel` 原即失敗且不推送，回應不變。極端競爭（assign 後、commit 前他人再改）以 commit 重讀為準。
- 執行過的指令與結果（全綠）：
  - `cargo test --manifest-path backend/Cargo.toml --lib kea::push --test kea_sync --test assignments --test asset_assignments --test ips` → lib 側 `kea::push` 5 passed、0 failed（整合測試因 filter 為 0）。
  - 同四個整合目標全跑：kea_sync 29（檔案一字未改）、assignments 12、asset_assignments 6、ips 26 passed、0 failed。
  - `cargo test --manifest-path backend/Cargo.toml` → 31 個 test 目標合計 477：471 passed、0 failed、6 ignored（`tests/kea_connectivity.rs` 既有需真機 Kea 者；`--lib` 213 passed 含新增 5）。
  - `cargo fmt --manifest-path backend/Cargo.toml` 後 `-- --check` → 通過；`cargo check --manifest-path backend/Cargo.toml --all-targets` → 無警告。
