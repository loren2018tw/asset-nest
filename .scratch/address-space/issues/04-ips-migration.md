# 04 — ips 改用 AddressSpace

Status: ready-for-agent
Blocked by: 03

## 目標

`ips` 的清單與狀態改由 `AddressSpace` 判定；分頁、排序、篩選與文案逐字不變。

## 範圍

- `backend/src/ips.rs`：
  - `list_v4`：建一次 `AddressSpace::new(subnet)?`；`status_of`（或等價函式）改吃 `&AddressSpace` 與 `Option<&ListedAssignment>`：已指派→用途字串（不變）；`InPool`→`in_pool`；`InExcluded`→`excluded`；`Host`→`available`。
  - 出界指派列（extras：「host 範圍列 ∪ 指派列」）以 `space.hosts()` 等價改寫。
  - `merged_conflicts`：票 03 已機械適配；本票整理為單次建 space 後傳遞，避免重複建構。
  - 刪除 `parse_pools`／`parse_exclusions` 與 `PoolRange`／`ExclusionRange` 型別。
  - `HostRange` 型別本體與 `host_range_*` 四個測試**保留到票 06**（屆時刪除），但本檔不再使用它於 `list_v4`。
  - v6 清單行為不動。
- 不動 `conflicts.rs`／`assignments.rs`／`kea`／`subnets.rs`。

## 驗收

- `cargo test --manifest-path backend/Cargo.toml --lib ips --test ips --test ip_registry_v6`
- `cargo test --manifest-path backend/Cargo.toml` 全綠
- `cargo fmt --manifest-path backend/Cargo.toml` 後 `--check` 綠；`cargo check` 無新警告

## 交付

1. commit：`位址空間：ips 改用 AddressSpace（票 04）`。
2. 本票檔：`Status: done`，`## Comments` 補記 commit hash 與摘要。
3. commit：`位址空間：票 04 補記實作與 commit hash`。
