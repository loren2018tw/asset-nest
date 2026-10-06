# 01 — 排除範圍資料模型與網段 CRUD

Status: ready-for-agent
Blocked by: —

## 目標

實作 `spec §3、§4`：migration、`Subnet` 型別與驗證、CRUD 讀寫。此票只做資料層與網段資源，不動 IP 清單、衝突、指派阻擋、候選端點與 CSV 匯出格式（後續票）。

## 範圍

- 新增 `backend/migrations/0010_subnet_exclusions.sql`（schema 見 spec §3）。
- `backend/src/subnets.rs`：
  - 列／API 型別：`ExclusionRow`、`ExclusionJoinRow`、`pub struct Exclusion`、`ExclusionInput`、`ValidExclusion`；`Subnet.exclusions`、`ValidSubnet.exclusions`。
  - `SubnetInput.exclusions`（`#[serde(default)]`）；`SubnetPatch.exclusions: Option<Vec<ExclusionInput>>`；`apply_to` 合併語意與 `pools` 對稱（`None` 沿用、`Some` 整批取代；沿用時把既有列轉回 `ExclusionInput`，含 note）。
  - `validate_fields` 新增 exclusions 參數與規則（spec §4 表 1–7；錯誤欄位命名照表：`exclusions`／`exclusions[i].start_ip`／`exclusions[i].end_ip`／`exclusions[i].note`）。
  - `fetch_exclusions`、`insert_exclusions`；`create`／`update`（`DELETE` 後整批插入，與 pools 同交易）；`get`、`list`、`list_full`、`find_by_address` 載入排除範圍；`into_subnet(pools, exclusions)`。
  - `list_full` 避免 N+1（比照 pools 的兩段查詢與分組）。
- 編譯修正（只補 `exclusions: vec![]`，不改測試語意）：`Subnet` struct literal 在 `backend/src/conflicts.rs`（測試 helper）、`backend/src/ips.rs`（測試 helper）、`backend/src/subnets.rs`（`into_subnet`、測試 `export_subnet`）。
- 測試：
  - `subnets.rs` 單元測試：spec §4 每條規則（v6 拒絕、端點必填／格式、順序、CIDR 內、彼此重疊含共端點、與 pool 重疊、note 含 `|` 拒絕、note 含 `#` 允許）；`valid_subnet` helper 更新。
  - `backend/tests/subnets.rs`：建立含排除範圍並讀回、PATCH 整批取代／清空、與 pool 重疊回 400（`validation_error`）、v6 帶排除範圍回 400。
  - `backend/tests/delete_linkage.rs`：刪除網段連動刪除 `subnet_exclusions`（比照既有 pools 連動測試）。

## 驗收

- `cargo test --manifest-path backend/Cargo.toml --lib subnets`
- `cargo test --manifest-path backend/Cargo.toml --test subnets --test delete_linkage`
- 最後跑一次 `pnpm test` 全綠；`cargo fmt --manifest-path backend/Cargo.toml -- --check`。
- 不改 `export_csv` 欄位（仍 6 欄）、不改 IP 清單與衝突行為。

## 注意

- 風格比照現有模組：中文註解、`ApiError::validation(...).field(...)`、sqlx 錯誤轉 `ApiError::internal`。
- 不要 `git commit`；不要動 `.scratch/` 內其他票。
