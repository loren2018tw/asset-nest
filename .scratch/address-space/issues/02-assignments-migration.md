# 02 — assignments 改用 AddressSpace

Status: ready-for-agent
Blocked by: 01

## 目標

`assignments::validate_address` 的外部簽章與錯誤文案逐字不變，內部改由 `AddressSpace` 判定（spec §呼叫端對映前三列）。

## 範圍

- `backend/src/assignments.rs`：
  - `validate_address(subnet, address, purpose, existing)` 簽章不變；內部建 `AddressSpace::new(subnet)?`：
    - v4、`existing = false`：`Outside`／`NonHost` →「位址 {a} 不在網段 {cidr} 的可用範圍內（network/broadcast 不可指派）」（field `address`＋detail `cidr` 照舊）；`InPool` →「位址 {a} 落在 DHCP 位址池內，不可指派」；`InExcluded` →「位址 {a} 落在排除範圍內，不可指派」；`Host` → 通過。
    - v4、`existing = true`：不重驗出界／落池／排除（現行語意不變）。
    - v6：`Outside` →「位址 {a} 不在網段 {cidr} 內」＋用途限 static；其餘照舊。
    - 地址族不符訊息不變。
  - 刪除 `is_in_pool`／`is_in_exclusion`（確認無其他使用者）；`parse_network` 與 `HostRange` import 若不再使用則一併刪除。
- 不動 `import.rs`（其遷移在票 05；匯入走 `validate_address` 公開路徑不受影響）。

## 驗收

- `cargo test --manifest-path backend/Cargo.toml --test assignments --test asset_assignments --test import`
- `cargo test --manifest-path backend/Cargo.toml` 全綠（訊息斷言見 `tests/assignments.rs`、`tests/asset_assignments.rs`）
- `cargo fmt --manifest-path backend/Cargo.toml` 後 `--check` 綠；`cargo check` 無新警告

## 交付

1. commit：`位址空間：assignments 改用 AddressSpace（票 02）`。
2. 本票檔：`Status: done`，`## Comments` 補記 commit hash 與摘要。
3. commit：`位址空間：票 02 補記實作與 commit hash`。
