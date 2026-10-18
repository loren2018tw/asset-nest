# 規格：位址空間（Address Space）——位址落點判定的單一 module

- 狀態：已定案（2026-10-19）；實作見票 01–06（完成後於本檔補記實作記錄）。
- 詞彙依 `GLOSSARY.md`（位址、網段、DHCP 位址池、排除範圍、可用／池內／排除、衝突）；架構詞彙依 codebase-design（module／interface／implementation／seam／adapter／locality／leverage）。
- 前置：`.scratch/asset-ip-management/`（v4 枚舉、狀態、衝突標記）、`.scratch/subnet-exclusions/`（排除範圍；ADR-0020）、`.scratch/kea-subnet-sync/`（Kea 網段層設定；ADR-0013）。
- 本規格是純重構：**不改任何對外行為、狀態、衝突碼、錯誤文案與排序**；唯一例外見 §錯誤（資料損壞、寫入驗證不可達路徑）。

## 目標

把「這個位址落在網段裡的哪裡？」收成一個 in-process deep module（`backend/src/address_space.rs`）：

- 呼叫端不再各自解析 `Subnet.pools`／`Subnet.exclusions`、比較區間、解析 CIDR；
- 三種區間表示法（`PoolRange`／`ExclusionRange`、`(Ipv4Addr, Ipv4Addr)`、`(u32, u32)`）收成一種 `Ipv4Range`；
- 落點語意只存在一處；ADR-0020 型的規則修訂只需改這裡。

## 背景（摩擦）

六個呼叫端各自實作同一條規則（現行程式碼）：

| 呼叫端 | 現況 |
|---|---|
| `backend/src/ips.rs` | `PoolRange`／`ExclusionRange`＋`parse_pools`／`parse_exclusions`＋`status_of`（`list_v4` 每次解析） |
| `backend/src/conflicts.rs` | 私有 `parse_pools`／`parse_exclusions`（`(Ipv4Addr, Ipv4Addr)`）；`detect`／`observed_codes` 各解析一次 |
| `backend/src/assignments.rs` | `is_in_pool`／`is_in_exclusion`（逐呼叫 parse）＋`HostRange::of` |
| `backend/src/ip_candidates.rs` | `(u32, u32)` 索引＋`in_ranges`＋自存 `HostRange` |
| `backend/src/import.rs` | `ipv4_rule_code`＝`HostRange`＋`is_in_pool`＋`is_in_exclusion`（只為挑錯誤碼） |
| `backend/src/kea/sync.rs` | `subnet_pool_ranges`（第三次解析 pool；錯誤型別 `String`） |

另：`HostRange`（`backend/src/ips.rs`）被 `assignments`／`import`／`ip_candidates`／`subnets` 直接使用；`agent/src/host_range.rs` 是它的逐行複製（不在本規格範圍，候選 6）。最近 80 個 commit 中 `ips.rs`／`conflicts.rs`／`IpListPage.vue` 是最高頻的共同變動組——這條規則就是熱點本身。

## 決策（grilling 定案，2026-10-19）

1. **界線**：module 擁有 CIDR 解析與容納（v4／v6）、v4 host 範圍、pool 區段、排除區段、單一位址落點。不擁有：指派用途→狀態字串（`status_of` 留在 `ips`，候選 5 再收）、衝突碼與文案（`conflicts` 對映）、MAC／觀測／Kea 形狀、**寫入端結構驗證**（`subnets::validate_fields` 不動；寫入阻擋與讀取解析是兩層，互不取代）。
2. **形態**：`AddressSpace::new(&Subnet) -> Result<Self, AddressSpaceError>`；純資料結構，不快取、不進 `AppState`、不碰 DB；在需要它的地方建一次、以 `&AddressSpace` 向下傳。
3. **錯誤**：`AddressSpaceError`（`Display`＋`From<AddressSpaceError> for ApiError`）；kea 計畫報告用 `Display`。文案見 §錯誤。
4. **名稱與住處**：`backend/src/address_space.rs`、型別 `AddressSpace`／`Placement`／`Ipv4Range`。`GLOSSARY.md` 不動（實作概念，非領域名詞）；不新增 ADR（純內部重整，ADR-0006／ADR-0020 續用）。
5. **v6**：module 擁有兩族的 CIDR 解析與容納；pool／排除／host 範圍仍 v4 限定（寫入端已擋）。v6 落點只有 `Outside`／`Host`。
6. **遷移**：一系列小步（票 01–06），每步 `cargo test` 全綠、訊息逐字不變；被取代的私有重複函式隨票刪除；`cargo check` 不得出現未使用警告。

## 介面（草案）

```rust
// backend/src/address_space.rs
pub struct AddressSpace { /* network、hosts、pools、exclusions 一次解析 */ }

impl AddressSpace {
    pub fn new(subnet: &Subnet) -> Result<Self, AddressSpaceError>;
    pub fn placement(&self, address: IpAddr) -> Placement; // 總函式、不失敗
    pub fn network(&self) -> IpNet;
    pub fn hosts(&self) -> Option<Ipv4Range>;   // v6 回 None
    pub fn pools(&self) -> &[Ipv4Range];
    pub fn exclusions(&self) -> &[Ipv4Range];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement { Outside, NonHost, InPool, InExcluded, Host }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Ipv4Range { /* u32 為底；端點皆含；建構子不公開 */ }
impl Ipv4Range {
    pub fn start(&self) -> Ipv4Addr;
    pub fn end(&self) -> Ipv4Addr;
    pub fn contains(&self, address: Ipv4Addr) -> bool;
    pub fn count(&self) -> u64;
    pub fn nth(&self, offset: u64) -> Option<Ipv4Addr>;
    pub fn iter(&self) -> impl Iterator<Item = Ipv4Addr> + '_;
}
```

- 優先序固定：`Outside > NonHost > InPool > InExcluded > Host`。
- host 範圍（v4）：`/31`、`/32` 全數列出；其餘扣 network／broadcast（現行 `HostRange::of` 語意）。
- 單一 enum 是忠實的：`SubnetPatch::apply_to` 把 pool／排除範圍對「最終 CIDR」整批驗證（`validate_fields`），且排除範圍不得與 pool 重疊（ADR-0020）——「網段外＋池內」等組合在儲存態不可達。

## 錯誤

```rust
pub enum AddressSpaceError {
    InvalidCidr(ipnet::AddrParseError),
    InvalidPool(std::net::AddrParseError),
    InvalidExclusion(std::net::AddrParseError),
    ReversedPool { start: String, end: String },
}
```

- `Display`：`InvalidCidr`→「網段 CIDR 格式錯誤：{source}」；`InvalidPool`→「pool 位址格式錯誤：{source}」；`InvalidExclusion`→「排除範圍位址格式錯誤：{source}」；`ReversedPool`→「pool 範圍顛倒：{start}-{end}」。
- `From<AddressSpaceError> for ApiError`＝`ApiError::internal(<context>, &error)`，context 為「網段 CIDR 格式錯誤」／「pool 位址格式錯誤」／「排除範圍位址格式錯誤」／「pool 範圍顛倒」。
- 建構子只驗證：可解析＋`start <= end`。**不**重驗 pool⊆CIDR、排除×pool 重疊等結構規則（寫入端職責）。
- kea 計畫錯誤字串＝`error.to_string()`。已知差異：資料損壞（寫入驗證不可達）時，kea 計畫的 pool 解析錯誤文案由「pool 起點格式異常：…」統一為「pool 位址格式錯誤：…」；「pool 範圍顛倒」逐字不變；無測試斷言這些路徑。

## 呼叫端對映（行為等價表）

| 呼叫端 | 現行判定 | 新判定（訊息逐字不變） |
|---|---|---|
| `assignments::validate_address`（v4、`existing = false`） | `!HostRange.contains` → 「位址 {a} 不在網段 {cidr} 的可用範圍內（network/broadcast 不可指派）」（field `address`＋detail `cidr`） | `Outside`／`NonHost` → 同一句 |
| 同上 | `is_in_pool` → 「位址 {a} 落在 DHCP 位址池內，不可指派」 | `InPool` → 同一句 |
| 同上 | `is_in_exclusion` → 「位址 {a} 落在排除範圍內，不可指派」 | `InExcluded` → 同一句 |
| 同上（v6） | `!network.contains` → 「位址 {a} 不在網段 {cidr} 內」＋用途限 static | `Outside` → 同一句 |
| `import::ipv4_rule_code` | `!HostRange.contains`／pool／排除，回 `Result` | `Outside`／`NonHost`→`ipv4_not_host`；`InPool`→`ipv4_in_pool`；`InExcluded`→`ipv4_in_exclusion`；`Host`→`invalid_ipv4`（純函式、不再回 `Result`） |
| `conflicts::detect` | 獨立 if：`IpOutOfSubnet`／`IpInPool`／`IpInExcludedRange` | 同一優先序對映三碼；`NonHost`／`Host` 無碼 |
| `conflicts::observed_codes` | 未指派且非 pool 非排除 → `ObservedOnUnassigned` | `InPool`／`InExcluded` 不標記；其餘標記 |
| `ip_candidates::is_unavailable` | `!hosts.contains \|\| 已指派 \|\| in pool \|\| in exclusions` | `placement != Host \|\| 已指派` |
| `ip_candidates::status_for` | 無所屬網段→用途→非 host→池內→排除→可用 | 同順序以 `placement` 表達 |
| `ips::status_of` | 已指派→用途；否則池內→排除→可用 | `InPool`→`in_pool`；`InExcluded`→`excluded`；`Host`→`available`；已指派照舊 |
| `ips::list_v4` 出界指派列 | 不在 host 範圍的指派仍列出（網段縮小） | `space.hosts()` 判定（等價） |
| `kea/sync::subnet_pool_ranges` | parse＋反轉檢查，`String` 錯誤 | `AddressSpace::new(...)?.pools()`；錯誤以 `Display`；範圍對映 `kea::http::Ipv4Range`（`start()`／`end()`） |
| `subnets::list` 摘要 | v4：`HostRange::of(&network).count()`；v6：`used` | v4：`space.hosts()` 的 count；v6 仍 `used` |

## 範圍（票）

| 票 | 內容 | Blocked by |
|---|---|---|
| 01 | module 與 `Ipv4Range`（含全部 module 測試） | — |
| 02 | `assignments` 遷移 | 01 |
| 03 | `conflicts` 遷移（含呼叫端機械適配） | 01 |
| 04 | `ips` 遷移 | 03 |
| 05 | `ip_candidates`＋`import` 遷移 | 01 |
| 06 | `kea/sync`＋`subnets` 摘要；`HostRange` 退場 | 05 |

## 測試決策

- 新 module 的單元測試（票 01）：落點優先序（含池／排除重疊時 pool 先——防禦性）、pool／排除端點皆含、network／broadcast＝`NonHost`、`/31`／`/32` 全 host、`0.0.0.0/0` 的 count、`nth`／`iter` 邊界、v6 `Outside`／`Host`、建構錯誤（非法 CIDR／pool／排除、起>迄）。原 `ips.rs` 的 `host_range_*` 四測之案例在此重寫；`ips.rs` 舊測於票 06 隨 `HostRange` 刪除。
- 行為不變的證據＝既有單元＋HTTP 測試全綠；訊息斷言在 `backend/tests/assignments.rs`、`backend/tests/asset_assignments.rs`、`backend/tests/subnet_import.rs`。
- 不新增 HTTP 測試（無新行為）。

## 明確不做

- 狀態／衝突合成搬入（候選 5）、`purpose_str`／`purpose_status` 重複收斂（候選 5）。
- `agent/src/host_range.rs` 共用（候選 6）。
- 寫入驗證整合（`subnets::validate_fields` 不動）。
- 任何 API／UI／訊息／排序／資料庫變更。

## 實作記錄

已實作（2026-10-19，票 01–06 全數完成）。

- 設計會談產出：`ee59907`（spec 與票單）。實作 commits：票 01 `f99b0c1`、票 02 `ebd68cd`、票 03 `7cb81e5`、票 04 `f0ee9a2`、票 05 `7b46ddb`、票 06 `aee1f6e`（各票補記見票檔 `## Comments`）。
- 終態：落點判定來源單一（`AddressSpace`）；`HostRange` 自 `backend/src` 完全退場（`grep -rn "HostRange" backend/src` 無命中）；六個呼叫端零自有解析。
- 驗收：`cargo test` 466 passed／0 failed／6 ignored（`kea_connectivity` 需真機 Kea，既有）；`cargo fmt --check` 綠；`cargo check --all-targets` 無警告。
- 與規格差異（皆屬 §錯誤允許的資料損壞、寫入驗證不可達路徑，無測試斷言）：
  1. 排除範圍起>迄亦回 `ReversedPool`（維持 `Ipv4Range` 起<=迄不變式；文案沿用「pool 範圍顛倒」）。
  2. 出界＋排除同時命中的損壞資料：舊版標兩碼，新版依優先序只標 `IpOutOfSubnet`；對應單元測試更名為 `out_of_subnet_outranks_exclusion_on_corrupt_data`。
  3. pool 解析錯誤文案統一為「pool 位址格式錯誤：…」（原 ips／kea 各異；kea 原為「pool 起點格式異常」）；kea 計畫遇損壞資料改進計畫錯誤欄位而非整個請求 500。
  4. `IpEntry.in_pool` 欄維持「原始 pool 成員判定」（`space.pools()`）：pool 可涵蓋 network／broadcast，該可達邊界下不採用落點 enum。
  5. 原 `host_range_*` 測試實為 3 個（非 4），已自 `ips.rs` 移除；module 端新增 15 個測試（票 01）。
- 後續（未做，屬另案）：候選 5（IP 清單投影）、候選 6（agent `host_range` 共用）。
