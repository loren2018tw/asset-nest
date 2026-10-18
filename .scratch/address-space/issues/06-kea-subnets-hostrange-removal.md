# 06 — kea/sync 與 subnets 摘要遷移；HostRange 退場

Status: ready-for-agent
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
