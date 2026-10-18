# 06 — kea/sync 與 subnets 摘要遷移；HostRange 退場

Status: done
Blocked by: 05

## 目標

最後兩個呼叫端改用 `AddressSpace`；`HostRange` 自 `backend/src` 完全退場（`agent/` 不動）。

## 範圍

- `backend/src/kea/sync.rs`：
  - 刪除 `subnet_pool_ranges`；計畫改以 `AddressSpace::new(&subnet)`，建構錯誤以 `AddressSpaceError` 的 `Display` 進計畫錯誤欄位（文案見 spec §錯誤：「pool 範圍顛倒：{start}-{end}」逐字不變；pool 解析錯誤統一為「pool 位址格式錯誤：…」）。
  - `desired_pools` 的範圍型別：以 `kea::http::Ipv4Range { start, end }` 對映（`start()`／`end()`），或等價最小改動；計畫輸出字串（`to_compact_string` 等）不變。
- `backend/src/subnets.rs`：`list` 的 v4 `total` 改 `space.hosts().count()`（v6 仍 `used`）；刪除 `HostRange` import。
- `backend/src/ips.rs`：刪除 `HostRange` 型別與 `host_range_*` 四個測試（案例已於票 01 在 module interface 重寫）。
- 確認全 `backend/src` 不再出現 `HostRange`。

## 驗收

- `cargo test --manifest-path backend/Cargo.toml --lib --test kea_sync --test subnets`
- `cargo test --manifest-path backend/Cargo.toml` 全綠
- `grep -rn "HostRange" backend/src` 無命中
- `cargo fmt --manifest-path backend/Cargo.toml` 後 `--check` 綠；`cargo check` 無新警告

## 交付

1. commit：`位址空間：kea 與 subnets 遷移、HostRange 退場（票 06）`。
2. 本票檔：`Status: done`，`## Comments` 補記 commit hash 與摘要。
3. commit：`位址空間：票 06 補記實作與 commit hash`。

## Comments

實作完成（2026-10-19）；commit `aee1f6e`（`位址空間：kea 與 subnets 遷移、HostRange 退場（票 06）`，含 `backend/src/kea/sync.rs`、`backend/src/subnets.rs`、`backend/src/ips.rs`，另加 `backend/src/address_space.rs` 的一行 doc 註解，見下）。

- 變更摘要：
  - `kea/sync.rs`：刪除 `subnet_pool_ranges`；`compute_plans` 改以 `AddressSpace::new(&subnet)` 建構一次（供 `desired_pools` 對映與後續衝突判定共用），建構錯誤以 `AddressSpaceError` 的 `Display` 進計畫錯誤欄位（維持「讀取網段 pool 失敗：」前綴；「pool 範圍顛倒：{start}-{end}」逐字不變；pool 解析錯誤統一為「pool 位址格式錯誤：…」）；`desired_pools` 以 `kea::http::Ipv4Range { start, end }` 對映（`start()`／`end()`），`to_compact_string`／`to_kea_string` 等計畫輸出字串不變；刪除迴圈內重複的 `AddressSpace::new(&subnet)?` 與隨之無用的 `Ipv4Addr` import。
  - `subnets.rs`：`list` 的 v4 `total` 改 `space.hosts().map_or(used, |hosts| hosts.count())`（v6 仍 `used`）；刪除 `HostRange` import 與 `subnet.cidr.parse::<IpNet>()` 解析。
  - `ips.rs`：刪除 `HostRange` 型別本體與其 `host_range_*` 測試（現行檔案共 3 個：`host_range_excludes_network_and_broadcast`、`host_range_lists_all_for_slash31_and_slash32`、`host_range_supports_any_prefix`；票文與 spec 稱「四個」係計數差異，歷史版本亦僅此 3 個）與隨之無用的 `Ipv4Net` import。
  - `address_space.rs`（超出票列檔案範圍，惟驗收 `grep -rn "HostRange" backend/src` 無命中所需）：`host_range` 的 doc 註解原引用 `ips::HostRange::of`，改為「沿用原 host 範圍規則」；僅註解一行，無行為變更。
- 差異與取捨：
  - 資料損壞路徑（寫入驗證不可達）：CIDR／排除範圍不可解析時，舊版在迴圈內以 `?` 讓整個計畫請求 500；新版統一進計畫錯誤欄位（「讀取網段 pool 失敗：{Display}」）並跳過該網段。屬 spec §錯誤允許的資料損壞例外，無測試斷言。
  - pool 解析錯誤文案由「pool 起點格式異常：…」／「pool 終點格式異常：…」統一為「pool 位址格式錯誤：…」（spec §錯誤明文）；「pool 範圍顛倒：…」與計畫輸出字串逐字不變。
- 執行過的指令與結果（全綠）：
  - `cargo test --manifest-path backend/Cargo.toml --lib --test kea_sync --test subnets` → lib 208＋kea_sync 29＋subnets 11 passed、0 failed、0 ignored。
  - `cargo test --manifest-path backend/Cargo.toml` → 31 個 test 目標合計 466 passed、0 failed、6 ignored（較票 05 基線 469 passed 少 3＝刪除的 `host_range_*` 測試；6 ignored 為 `tests/kea_connectivity.rs` 既有需真機 Kea 者）。
  - `grep -rn "HostRange" backend/src` → 無命中（exit 1；`agent/` 未動）。
  - `cargo fmt --manifest-path backend/Cargo.toml` 後 `-- --check` → 通過；`cargo check --manifest-path backend/Cargo.toml --all-targets` → 無警告。
