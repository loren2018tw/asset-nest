# 04 — ips 改用 AddressSpace

Status: done
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

## Comments

實作完成（2026-10-19）；commit `f0ee9a2`（`位址空間：ips 改用 AddressSpace（票 04）`，只含 `backend/src/ips.rs`）。

- 變更摘要：
  - `list()`：改為 `AddressSpace::new(subnet)?` 建一次、以 `space.network()` 分派 v4／v6；`list_v4`／`list_v6` 皆接收 `&AddressSpace`（v6 不再自行建構，每請求仍只建一次）。`parse_network` 無他用、隨票刪除。
  - `list_v4`：不再自行 `parse_pools`／`parse_exclusions`；host 範圍改 `space.hosts()`（`expect("v4 網段必有 host 範圍")`，僅由 v4 分派進入）；出界指派列判定（不在 host 範圍者仍列出）語意與舊版逐字等價。
  - `status_of(space: &AddressSpace, address, assignment: Option<&ListedAssignment>)`：已指派→用途字串（不變）；未指派以 `space.placement()` 對映：`InPool`→`in_pool`、`InExcluded`→`excluded`；`Host`（防禦性含 `Outside`／`NonHost`）→`available`。
  - `entry_v4` 的 `in_pool` 欄維持 pool 成員判定（`space.pools()`）；`matches_v4_filters` 的 `unassigned_seen` 之「非池內、非排除」改以 `placement` 表達（未指派列恆為 host 範圍列，兩者等價）。
  - `merged_conflicts(space, assignments, presence)`：改收既有 space（票 03 為每次自建），`by_address` 與 `observed_codes` 共用；搭配上列變更，`list_v4` 每次請求總計只建構一次位址空間。
  - 刪除 `PoolRange`／`ExclusionRange` 型別與 `parse_pools`／`parse_exclusions`／`parse_stored_address`；`HostRange` 型別本體與 `host_range_*` 四個測試保留（票 06 退場），`list_v4` 不再使用。
  - v6：`list_v6` 改收共用 space；分頁／排序／篩選／狀態與錯誤處理零變更（`tests/ip_registry_v6.rs` 5 測與 lib 內 v6 測皆綠）。
- 差異與取捨：
  - 資料損壞路徑（寫入驗證不可達）：pool／排除範圍位址不可解析時，500 訊息由舊「位址格式錯誤」統一為「pool 位址格式錯誤」／「排除範圍位址格式錯誤」；起>迄（顛倒 pool／排除範圍）舊版僅永不命中、新版回 500「pool 範圍顛倒」。皆屬 spec §錯誤允許的資料損壞例外，無測試斷言。
  - `in_pool` 欄刻意**不**改以落點（`placement == InPool`）判定：pool 允許涵蓋 network/broadcast（寫入驗證僅要求 pool⊆CIDR），若改用落點，該可達邊界（如既有指派因 CIDR 縮小成為 network、pool 覆蓋之）會使 `in_pool` 由 `true` 翻為 `false`；票文未列此欄，故維持成員判定以求逐字不變。狀態字串本身不受影響。
  - 未指派列的 `Outside`／`NonHost` 防禦性對映維持舊版語意（非池內非排除→`available`），行為不變。
- 執行過的指令與結果（全綠）：
  - `cargo test --manifest-path backend/Cargo.toml --lib ips --test ips --test ip_registry_v6` → lib 37 passed、0 failed（名篩選 `ips` 對整合目標為 0 筆，見票 03 先例）；另以無名篩選補跑 `--test ips --test ip_registry_v6` → 26＋5 passed、0 failed。
  - `cargo test --manifest-path backend/Cargo.toml` → 31 個 test 目標合計 469 passed、0 failed、6 ignored（`tests/kea_connectivity.rs` 既有需真機 Kea 者；與票 02／03 基線一致）。
  - `cargo fmt --manifest-path backend/Cargo.toml` 後 `-- --check` → 通過。
  - `cargo check --manifest-path backend/Cargo.toml`（另含 `--all-targets`）→ 無警告。
