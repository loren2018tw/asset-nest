# 03 — conflicts 改用 AddressSpace

Status: done
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

## Comments

實作完成（2026-10-19）；commit `7cb81e5`（`位址空間：conflicts 改用 AddressSpace（票 03）`，含 `backend/src/conflicts.rs`、`backend/src/ips.rs`、`backend/src/subnets.rs`、`backend/src/kea/sync.rs`）。

- `backend/src/conflicts.rs`：
  - `detect(space: &AddressSpace, assignments)`：改以 `space.placement()` 對映三碼（`Outside`→`IpOutOfSubnet`、`InPool`→`IpInPool`、`InExcluded`→`IpInExcludedRange`；`NonHost`／`Host` 無碼）；`DuplicateHwAddress` 照舊並存、碼序不變。
  - `by_address(space, assignments)` 同步調整。
  - `observed_codes(space, assignments, presence)`：`InPool`／`InExcluded` 不標記、其餘標記；觀測現況位址仍以 `Ipv4Addr` 解析（錯誤文案「觀測現況位址格式錯誤」不變）。
  - `warnings_for(subnet, assignments, address)` 簽章與訊息不變（訊息仍用 `subnet.cidr`）；內部 `AddressSpace::new(subnet)?`。
  - 刪除私有 `parse_pools`／`parse_exclusions`；四碼定義與 `message_of` 逐字不變。
  - 單元測試：新增 `space_of(&Subnet)` helper，`detect`／`by_address`／`observed_codes` 呼叫點改為先建空間再呼叫；`warnings_for` 呼叫點不動。
- 機械適配的呼叫端（各處在呼叫點建 `AddressSpace::new(subnet)?`，錯誤經 `From<AddressSpaceError> for ApiError` 進 500，與現行 `ApiError::internal` 同等）：
  - `ips.rs::merged_conflicts`：建一次 space，`by_address` 與 `observed_codes` 共用。
  - `ips.rs::list_v6`：`cargo check` 找出的同檔第二處 `by_address`，同樣先建 space。
  - `subnets.rs::list`：衝突數改 `detect(&space, &assignments)`。
  - `kea/sync.rs::compute_plans`：計畫的 `by_address` 改 `AddressSpace::new(&subnet)?`。
- 差異與取捨：
  - 既有單元測試 `ip_in_excluded_range_orders_with_other_codes` 的唯一斷言變更：其案例為排除範圍在 CIDR 外（資料損壞、寫入端不可達——`validate_fields` 擋排除範圍⊆CIDR）；依 spec §呼叫端對映的優先序 `Outside` 先於 `InExcluded`，出界保留只標 `IpOutOfSubnet`（仍與 `DuplicateHwAddress` 依固定順序並存），測試更名為 `out_of_subnet_outranks_exclusion_on_corrupt_data`。其餘斷言不動。
  - 資料損壞路徑（pool／排除範圍在 CIDR 外、起>迄）語意變更：舊 `detect` 不檢查起>迄（顛倒範圍只是永不命中），新 `AddressSpace::new` 回 500「pool 範圍顛倒」；出界且落 pool／排除的位址舊標兩碼、新依優先序標一碼。皆屬 spec §錯誤允許的資料損壞例外，無測試斷言。
  - `detect`／`observed_codes` 不再各自解析；`merged_conflicts` 由兩次解析降為一次。
- 執行過的指令與結果（全綠）：
  - `cargo test --manifest-path backend/Cargo.toml --lib conflicts --test conflicts` → lib 11 passed、0 failed（整合目標測試名不含 `conflicts` 字串，篩選後 0 筆）。
  - `cargo test --manifest-path backend/Cargo.toml` → 31 個 test 目標合計 469 passed、0 failed、6 ignored（`tests/kea_connectivity.rs` 既有需真機 Kea 者）。
  - `cargo fmt --manifest-path backend/Cargo.toml` 後 `-- --check` → 通過。
  - `cargo check --manifest-path backend/Cargo.toml` → 無警告。
