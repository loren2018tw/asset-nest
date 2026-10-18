# 05 — ip_candidates 與 import 改用 AddressSpace

Status: ready-for-agent
Blocked by: 01

## 目標

候選查詢與匯入列驗證改由 `AddressSpace` 判定；行為與錯誤碼逐字不變。

## 範圍

- `backend/src/ip_candidates.rs`：
  - `IndexedSubnet` 以 `space: AddressSpace` 取代 `hosts`／`pools`／`exclusions` 欄位；`network` 由 `space.network()` 取得或等價改寫；`find` 與排序鍵行為不變。
  - `is_unavailable`＝`space.placement(...) != Placement::Host || 已指派`；刪除 `in_ranges`；本地 `parse_stored_address` 若無他用則刪除。
  - `status_for` 判定順序不變（無所屬網段→用途→非 host→池內→排除→可用），以 `placement` 表達。
- `backend/src/import.rs`：
  - `ipv4_rule_code(space: &AddressSpace, address: Ipv4Addr) -> &'static str`（不再回 `Result`）：`Outside`／`NonHost`→`ipv4_not_host`；`InPool`→`ipv4_in_pool`；`InExcluded`→`ipv4_in_exclusion`；`Host`→`invalid_ipv4`。
  - 呼叫端（`analyze` 迴圈）在該網段 context 建 `AddressSpace`；建構失敗的處理比照現行該處（`ApiError`）。
  - 刪除本檔 `HostRange` import；`IpNet` 若無他用一併清理。
- `backend/src/assignments.rs`：刪除 `is_in_pool`／`is_in_exclusion`（票 02 保留，因 `import::ipv4_rule_code` 直接呼叫；本票遷移後唯一使用者消失）與隨之無用的 import／helper。
- 不動 `kea/sync.rs`／`subnets.rs`／`ips.rs`。

## 驗收

- `cargo test --manifest-path backend/Cargo.toml --test ip_candidates --test import --test subnet_import`
- `cargo test --manifest-path backend/Cargo.toml` 全綠（錯誤碼斷言見 `tests/subnet_import.rs` 與 `tests/import.rs`）
- `cargo fmt --manifest-path backend/Cargo.toml` 後 `--check` 綠；`cargo check` 無新警告

## 交付

1. commit：`位址空間：ip_candidates 與 import 改用 AddressSpace（票 05）`。
2. 本票檔：`Status: done`，`## Comments` 補記 commit hash 與摘要。
3. commit：`位址空間：票 05 補記實作與 commit hash`。
