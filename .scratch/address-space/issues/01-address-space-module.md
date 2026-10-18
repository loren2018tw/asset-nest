# 01 — 位址空間 module 與 Ipv4Range

Status: done
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

## Comments

實作完成（2026-10-19）；commit `f99b0c1`（`位址空間：module 與 Ipv4Range（票 01）`，只含 `backend/src/lib.rs` 與 `backend/src/address_space.rs`）。

- 變更摘要：
  - `backend/src/lib.rs`：掛 `pub mod address_space;`（字母序首位）。
  - `backend/src/address_space.rs`（新增）：
    - `AddressSpace::new(&Subnet)`：CIDR 解析一次；v4 推導 host 範圍（`/31`、`/32` 全數列出，其餘扣 network／broadcast，語意同 `ips::HostRange::of`）、解析 pool／排除範圍；v6 忽略 pool／排除（寫入端已擋）。只驗證可解析與起點<=終點；不重驗 pool⊆CIDR、排除×pool 重疊。
    - `placement(&IpAddr) -> Placement`：總函式；v4 依 `Outside > NonHost > InPool > InExcluded > Host`；v6 只有 `Outside`／`Host`；地址族不符視為 `Outside`。
    - 存取子：`network()`、`hosts() -> Option<Ipv4Range>`（v6 `None`）、`pools()`／`exclusions() -> &[Ipv4Range]`（保持輸入順序）。
    - `Ipv4Range`：u32 為底、端點皆含；`start`／`end`／`contains`／`count`（u64 計算）／`nth`／`iter`；derive `Debug/Clone/Copy/PartialEq/Eq/Hash/PartialOrd/Ord`；建構子不公開（module 內部）。
    - `AddressSpaceError`（`Debug`＋`Display`）與 `From<AddressSpaceError> for ApiError`（`ApiError::internal(<句首>, &error)`）；文案照 spec §錯誤。
  - 測試：module 內 15 個——落點優先序、壞資料防禦（network 先於 pool／網段外先於排除）、pool×排除重疊時 pool 先、pool／排除端點皆含、network／broadcast＝`NonHost`、`/31`／`/32` 全 host、`0.0.0.0/0` count、`nth`／`iter` 邊界（超出回 `None`、起訖一致）、v6 `Outside`／`Host`（含忽略 pool／排除與地址族不符）、建構錯誤（非法 CIDR／pool／排除、起>迄含 `ReversedPool` 文案與原文保留）、`From` 對映四個 context。
- 執行過的指令與結果（全綠）：
  - `cargo test --manifest-path backend/Cargo.toml --lib address_space` → 15 passed、0 failed。
  - `cargo test --manifest-path backend/Cargo.toml` → 全 30 個 test binary 合計 469 passed、0 failed、6 ignored（`tests/kea_connectivity.rs` 既有需 Kea 環境者）；其中 `--lib` 211 passed。
  - `cargo fmt --manifest-path backend/Cargo.toml` 後 `-- --check` → 通過。
  - `cargo check --manifest-path backend/Cargo.toml` → 無警告。
- 取捨：
  - 排除範圍起點大於終點亦回 `ReversedPool`（規格建構子「只驗證可解析＋start <= end」；`ReversedPool` 為唯一顛倒變體）。此為寫入驗證不可達路徑，對既有呼叫端行為無影響。
  - `From` 的 context 斷言以 `ApiError::message()`（`pub(crate)`）驗證；`Display` 文案另以 `to_string()` 斷言。
