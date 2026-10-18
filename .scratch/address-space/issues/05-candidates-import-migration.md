# 05 — ip_candidates 與 import 改用 AddressSpace

Status: done
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

## Comments

實作完成（2026-10-19）；commit `7b46ddb`（`位址空間：ip_candidates 與 import 改用 AddressSpace（票 05）`，含 `backend/src/ip_candidates.rs`、`backend/src/import.rs`、`backend/src/assignments.rs`）。

- 變更摘要：
  - `ip_candidates::IndexedSubnet`：`hosts`／`pools`／`exclusions` 三欄刪除，改為 `space: AddressSpace`；`build` 以 `AddressSpace::new(subnet)?` 建構（建構失敗經 `From<AddressSpaceError> for ApiError` 成為分析錯誤，context 句與舊版相同）。`network: Ipv4Net` 保留為排序與二分搜尋鍵，值由 `space.network()` 於建構時取得（`build` 仍過濾 v6 網段）。
  - `is_unavailable`＝`space.placement(...) != Placement::Host || 已指派`；`status_for` 未指派分支以 `placement` 對映：`Outside`／`NonHost`→`out_of_subnet`、`InPool`→`in_pool`、`InExcluded`→`excluded`、`Host`→`available`，判定順序與 `query_status` 各狀態逐字不變。
  - 刪除 `in_ranges` 與 `parse_stored_address`（本票後無使用者）。
  - `import::ipv4_rule_code(space: &AddressSpace, address: Ipv4Addr) -> &'static str`：`Outside`／`NonHost`→`ipv4_not_host`、`InPool`→`ipv4_in_pool`、`InExcluded`→`ipv4_in_exclusion`、`Host`→`invalid_ipv4`；呼叫端（`validate_row` 的 `validate_address` `Err` 分支，該網段 context）建 `AddressSpace` 後傳入，建構失敗如舊以 `?` 成為分析錯誤。刪除 `HostRange` 與 `IpNet` import（`IpNet` 於本檔已無他用）。
  - `assignments`：刪除 `is_in_pool`／`is_in_exclusion`（唯一使用者消失）與隨之無用的 `Ipv4Addr` import；`validate_address` 不受影響。
- 差異與取捨：
  - 資料損壞路徑（寫入驗證不可達）：pool／排除範圍位址不可解析時，`ip_candidates` 的 500 來源錯誤改為 `AddressSpaceError`（context 句不變）；起>迄（顛倒 pool／排除範圍）舊版僅永不命中、新版回 500「pool 範圍顛倒」。屬 spec §錯誤允許的資料損壞例外，無測試斷言。
  - `ipv4_rule_code` 改吃既有 `&AddressSpace`，呼叫端只在「`validate_address` 已失敗」時建構（必成功），不增加快樂路徑成本；`AddressSpace::new` 對 v6 網段早退不解析 pool／排除，與舊版跳過順序一致。
  - `is_unavailable` 的 OR 語意等價（不可用集合＝非 host ∪ 已指派 ∪ 池內 ∪ 排除）；`find` 與排序鍵行為逐字不變。
- 執行過的指令與結果（全綠）：
  - `cargo test --manifest-path backend/Cargo.toml --test ip_candidates --test import --test subnet_import` → 6＋19＋18 passed、0 failed。
  - `cargo test --manifest-path backend/Cargo.toml` → 31 個 test 目標合計 469 passed、0 failed、6 ignored（`tests/kea_connectivity.rs` 既有需真機 Kea 者；與票 02／03／04 基線一致）。
  - `cargo fmt --manifest-path backend/Cargo.toml` 後 `-- --check` → 通過。
  - `cargo check --manifest-path backend/Cargo.toml --all-targets` → 無警告。
