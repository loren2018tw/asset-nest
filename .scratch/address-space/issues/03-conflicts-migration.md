# 03 — conflicts 改用 AddressSpace

Status: ready-for-agent
Blocked by: 01

## 目標

`conflicts` 的偵測改由 `AddressSpace` 判定（spec §呼叫端對映）；呼叫端在同一票內機械適配，行為不變。

## 範圍

- `backend/src/conflicts.rs`：
  - `detect(space: &AddressSpace, assignments: &[ListedAssignment])`；`by_address` 同簽章調整。
  - `observed_codes(space: &AddressSpace, assignments, presence)`：`InPool`／`InExcluded` 不標記 `ObservedOnUnassigned`；其餘標記。
  - `warnings_for(subnet: &Subnet, assignments, address)` 簽章不變（訊息需要 `subnet.cidr`）；內部建 `AddressSpace`。
  - 刪除私有 `parse_pools`／`parse_exclusions`；`message_of` 與四碼定義不變。
  - 單元測試 helper 改走新簽章；斷言不變。
- 機械適配呼叫端（只換呼叫形態、不改行為；以 `cargo check` 找齊）：
  - `ips::merged_conflicts`、`subnets::list`、`kea/sync` 計畫——各處建 `AddressSpace::new(subnet)`，錯誤處理與現行同等（`?` 進 `ApiError::internal`）。

## 驗收

- `cargo test --manifest-path backend/Cargo.toml --lib conflicts --test conflicts`
- `cargo test --manifest-path backend/Cargo.toml` 全綠（訊息與衝突碼斷言不變）
- `cargo fmt --manifest-path backend/Cargo.toml` 後 `--check` 綠；`cargo check` 無新警告

## 交付

1. commit：`位址空間：conflicts 改用 AddressSpace（票 03）`。
2. 本票檔：`Status: done`，`## Comments` 補記 commit hash 與摘要（含適配的呼叫端清單）。
3. commit：`位址空間：票 03 補記實作與 commit hash`。
