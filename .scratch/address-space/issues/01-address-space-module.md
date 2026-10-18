# 01 — 位址空間 module 與 Ipv4Range

Status: ready-for-agent
Blocked by: —

## 目標

依 `spec` §介面／§錯誤，新增 `backend/src/address_space.rs`：`AddressSpace`、`Placement`、`Ipv4Range`、`AddressSpaceError`。本票只新增 module 與其測試，**不動任何呼叫端**（後續票遷移）。

## 範圍

- `backend/src/lib.rs`：掛 `pub mod address_space;`（依字母序，於 `pub mod assets;` 之前）。
- `backend/src/address_space.rs`：
  - `AddressSpace::new(&Subnet)`：解析 CIDR（`IpNet`）、host 範圍（v4；`/31`／`/32` 全數列出，其餘扣 network／broadcast）、`Subnet.pools`／`Subnet.exclusions`（v4；v6 忽略——寫入端已擋）；失敗回 `AddressSpaceError`。
  - `placement(&self, IpAddr) -> Placement`：總函式、不失敗；優先序 `Outside > NonHost > InPool > InExcluded > Host`；v6 只有 `Outside`／`Host`。
  - `network() -> IpNet`、`hosts() -> Option<Ipv4Range>`（v6 `None`）、`pools()／exclusions() -> &[Ipv4Range]`。
  - `Ipv4Range`：u32 為底、端點皆含；`start()`／`end()`（回 `Ipv4Addr`）、`contains`、`count`、`nth`、`iter`；derive `Debug/Clone/Copy/PartialEq/Eq/Hash/PartialOrd/Ord`；建構子不公開。
  - `AddressSpaceError`（含 `Display`）＋`impl From<AddressSpaceError> for ApiError`；文案與 context 照 spec §錯誤。
- 測試（module 內 `#[cfg(test)]`；`Subnet` literal helper 比照 `conflicts.rs`／`ips.rs` 測試）：
  - 落點優先序（含防禦性：pool 與排除同時命中時 `InPool` 先）；
  - pool／排除端點皆含；
  - network／broadcast＝`NonHost`；
  - `/31`、`/32` 全 host；
  - `0.0.0.0/0` 的 count；
  - `nth`／`iter` 邊界（超出回 `None`、起訖一致）；
  - v6：CIDR 內 `Host`、外 `Outside`；
  - 建構錯誤：非法 CIDR／pool／排除、`start > end`（`ReversedPool` 的 Display 為 `pool 範圍顛倒：{start}-{end}`）。
- 不動 `ips`／`assignments`／`conflicts`／`ip_candidates`／`import`／`kea`／`subnets` 任何檔案（除 `lib.rs`）。

## 驗收

- `cargo test --manifest-path backend/Cargo.toml --lib address_space`
- `cargo test --manifest-path backend/Cargo.toml` 全綠（無行為變更）
- `cargo fmt --manifest-path backend/Cargo.toml` 後 `--check` 綠；`cargo check` 無新警告

## 交付

1. commit：`位址空間：module 與 Ipv4Range（票 01）`（只含 `lib.rs` 與 `address_space.rs`）。
2. 本票檔：`Status: done`，`## Comments` 補記 commit hash 與摘要（含測試數）。
3. commit：`位址空間：票 01 補記實作與 commit hash`。
