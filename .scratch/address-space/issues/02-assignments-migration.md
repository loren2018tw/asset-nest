# 02 — assignments 改用 AddressSpace

Status: done
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

## Comments

實作完成（2026-10-19）；commit `ebd68cd`（`位址空間：assignments 改用 AddressSpace（票 02）`，只含 `backend/src/assignments.rs`）。

- 變更摘要：
  - `validate_address(subnet, address, purpose, existing)`：簽章不變；內部先 `AddressSpace::new(subnet)?`，地址族分派改以 `space.network()`：
    - v4、`existing = false`：依 `space.placement()`——`Outside`／`NonHost`→「位址 {a} 不在網段 {cidr} 的可用範圍內（network/broadcast 不可指派）」（field `address`＋detail `cidr`）；`InPool`→「位址 {a} 落在 DHCP 位址池內，不可指派」；`InExcluded`→「位址 {a} 落在排除範圍內，不可指派」；`Host`→通過。訊息與細節欄位逐字不變。
    - v4、`existing = true`：不重驗出界／落池／排除（語意不變）。
    - v6：`Outside`→「位址 {a} 不在網段 {cidr} 內」＋用途限 static（訊息不變）。
    - 地址族不符訊息不變。
  - `HostRange` import 刪除（檔案內已無他用）。`parse_network` 保留——`register` 仍用（v6 端點適用性檢查）。`IpNet` 保留。
  - `import.rs` 未動；其 `validate_address` 呼叫走公開路徑，訊息與行為不變。
- 差異與取捨：
  - `is_in_pool`／`is_in_exclusion` **未刪**：票文條件為「確認無其他使用者」，但 `import.rs::ipv4_rule_code` 仍直接呼叫兩者（票 05 才遷移），且本票不得改動 `import.rs`；刪除會使建置失敗。兩函式原樣保留，隨票 05 退場。
  - `AddressSpace::new` 建構即解析 pool／排除範圍：v4 `existing = true` 且資料損壞（pool／排除位址不可解析、起>迄）時，現行不解析（不擋）、新版回 500「pool 位址格式錯誤」／「pool 範圍顛倒」。屬 spec §錯誤的資料損壞例外（寫入驗證不可達）；正常資料路徑與訊息逐字不變，無測試觸及此路徑。
- 執行過的指令與結果（全綠）：
  - `cargo test --manifest-path backend/Cargo.toml --test assignments --test asset_assignments --test import` → 12＋6＋19 = 37 passed、0 failed。
  - `cargo test --manifest-path backend/Cargo.toml` → 全 31 個 test 目標（30 binary＋doc-tests）合計 469 passed、0 failed、6 ignored（`tests/kea_connectivity.rs` 既有需 Kea 環境者）；其中 `--lib` 211 passed。
  - `cargo fmt --manifest-path backend/Cargo.toml` 後 `-- --check` → 通過。
  - `cargo check --manifest-path backend/Cargo.toml` → 無警告。
