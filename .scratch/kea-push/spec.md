# 規格：Kea 推送（kea/push）——宣告異動推送的單一 module

- 狀態：已定案（2026-10-19）；架構審視候選 2 的設計會談產出；實作見票 01–04（完成後於本檔補記實作記錄）。
- 詞彙依 `GLOSSARY.md`（推送、受管網段、保留、指派、報廢）；架構詞彙依 codebase-design（module／interface／implementation／seam／adapter／locality／leverage）。
- 前置：`docs/adr/0011`（保留推送與完整同步）、`docs/adr/0027`（刪除與 MAC 編輯即時推送）、`docs/adr/0013`（網段層設定）、`docs/adr/0023`（同步建立網段）。**本規格不改任何對外行為**；唯一例外見 §損壞資料路徑。

## 目標

把「哪些異動要告知 Kea、異動前要擷取什麼、以什麼順序推、結果怎麼回」收成一個 in-process module（`backend/src/kea/push.rs`）：

- route 只剩三步：`capture_*`（本地異動前）→ 本地異動 → `commit_*`（異動後）；
- 觸發規則、捕捉查詢、逐筆推送順序、`KeaSync` 成形只存在一處；ADR-0027 型的修訂只改這個 module；
- 完整同步（`kea/sync.rs`）不動。

## 背景（摩擦）

七個呼叫點、五個 route module——規則在 `kea::sync::after_assignment_change`，但**捕捉查詢與判斷散在 route**：

| 呼叫端 | 現況重複 |
|---|---|
| `api/ips.rs` PUT 指派 | `assignments::find` 取 before → `assign` → `after_assignment_change` |
| `api/ips.rs` DELETE 指派 | 同型再一份 |
| `api/asset_assignments.rs` | `subnets::find_by_address`＋`assignments::find` 取 before → 同型第三份 |
| `api/interfaces.rs` PATCH | `if existing.mac != interface.mac` → 列保留 → `push_reservation_updates` |
| `api/interfaces.rs` DELETE | 列保留 → 刪 → `push_reservation_deletes` |
| `api/assets.rs` DELETE | 列介面→列保留 → 刪 → `push_reservation_deletes`（與下行逐字相同） |
| `api/disposed_assets.rs` 報廢 | 同上，再一份 |

測試只能經 HTTP＋stub Kea 驗證（`backend/tests/kea_sync.rs`，29 測）；推送規則本身沒有直接入口。

## 決策（grilling 定案，2026-10-19）

1. **邊界與形態**：兩段式 capture/commit；route 執行本地異動與 404／出借中判斷，module 擁有其餘一切。順序以型別與命名表達（pending 只能餵給對應 commit；capture 必在本地異動前）。
2. **after 狀態**：`commit_assignment` 自行重讀現況（route 不傳 after）；`commit_interface_change` 自行比較 MAC（未變即 `None`）。
3. **捕捉輸入**：`capture_assignment` 以位址（`IpAddr`）自行找網段；找不到＝永不推送。捕捉函式**永不 404**。
4. **Kea seam**：module 內部 `trait KeaPush { upsert; delete }`；正式 adapter 包 `http::Client`，測試用 in-process fake（internal seam，不對外公開）；公開函式仍收 `Option<&Client>`，route 零負擔。
5. **範圍**：五個家族（指派設定／取消／資產端移轉、刪資產、刪介面、報廢、MAC 編輯）；完整同步不動；舊 API 退場。
6. **名稱與紀錄**：`backend/src/kea/push.rs`；`KeaSync` 移入；pending 型別 `PendingAssignment`／`PendingDeletes`／`PendingInterfaceChange`。不新增 ADR（ADR-0011／0027 續用）；`GLOSSARY.md` 不動。

## 介面（草案）

```rust
// backend/src/kea/push.rs
pub struct KeaSync { pub status: &'static str, pub message: Option<String> } // 自 sync.rs 移入，Serialize 不變

// 捕捉（本地異動前呼叫；永不 404）
pub async fn capture_assignment(db, address: IpAddr) -> Result<PendingAssignment, ApiError>;
pub async fn capture_asset_deletion(db, asset_id: i64) -> Result<PendingDeletes, ApiError>;
pub async fn capture_interface_deletion(db, interface_id: i64) -> Result<PendingDeletes, ApiError>;
pub async fn capture_interface_change(db, interface_id: i64) -> Result<PendingInterfaceChange, ApiError>;

// 推送（本地異動後呼叫）
pub async fn commit_assignment(db, kea: Option<&Client>, pending) -> Result<Option<KeaSync>, ApiError>;
pub async fn commit_deletions(db, kea, pending) -> Result<Vec<KeaSync>, ApiError>;
pub async fn commit_interface_change(db, kea, pending) -> Result<Option<Vec<KeaSync>>, ApiError>;
```

- `PendingAssignment { subnet: Option<Subnet>, address: String, before: Option<Assignment> }`（`before` 為 `assignments::find` 的當下值）。
- `PendingDeletes { reservations: Vec<Assignment> }`（一律為 `purpose = "reservation"`，順序＝查詢序 `id ASC`）。
- `PendingInterfaceChange { interface_id: i64, mac_before: Option<String>, reservations: Vec<Assignment> }`。
- 欄位不公開；型別 `pub` 僅供 route 持有。

**內部 port 與 adapter**（`push.rs` 私有）：

```rust
trait KeaPush {
    async fn upsert(&self, subnet_id: i64, record: &ReservationRecord) -> Result<(), KeaError>;
    async fn delete(&self, subnet_id: i64, address: &str) -> Result<(), KeaError>;
}
struct HttpPush<'a>(&'a Client);
```

- `HttpPush::upsert`＝`reservation_del`（結果忽略）→ `reservation_add` → `config_write`（現行語意）。
- `HttpPush::delete`＝`reservation_del`；僅實際刪除（回 `true`）才 `config_write`（現行語意）。
- 公開 commit 函式以 `HttpPush` 包 `Option<&Client>` 後呼叫 internal 版本；module 測試直接以 `FakePush` 呼叫 internal 版本。

## 觸發規則（逐字沿用）

- 受管網段＝v4＋`kea_subnet_id`；未受管或未設 client → 不互動、不產生同步結果。
- `commit_assignment`：`after` 為保留 → upsert（介面無 MAC → `KeaError::Data("保留位址 {a} 的介面缺少 MAC")` → failed）；`before` 為保留、`after` 非保留（轉 static／取消）→ delete；兩者皆非 → `None`。
- `commit_deletions`：逐筆（順序不變）刪除；未受管跳過；失敗逐筆記 failed、續行。
- `commit_interface_change`：MAC 未變 → `None`；變更 → 對該介面保留逐筆以新 MAC upsert；空 → `None`。
- Kea 失敗 → `KeaSync{failed}`（訊息＝`KeaError::to_string()`）；DB 錯誤 → `ApiError`；不重試、不落地。

## 端點對映（JSON 形狀不變）

| 端點 | capture → 本地異動 → commit | 回應欄位 |
|---|---|---|
| `PUT /subnets/{id}/ips/{address}/assignment` | `capture_assignment` → `assignments::assign` → `commit_assignment` | `kea_sync`（單筆 optional） |
| `DELETE /subnets/{id}/ips/{address}/assignment` | `capture_assignment` → `assignments::cancel` → `commit_assignment` | 同上 |
| `PUT /assets/{id}/assignments` | `capture_assignment` → `assign_for_asset` → `commit_assignment` | 同上 |
| `PATCH /interfaces/{id}` | `capture_interface_change` → `interfaces::update` → `commit_interface_change` | optional 陣列 |
| `DELETE /interfaces/{id}` | `capture_interface_deletion` → `interfaces::delete` → `commit_deletions` | 陣列（可空） |
| `DELETE /assets/{id}` | `capture_asset_deletion` → `assets::delete` → `commit_deletions` | 同上 |
| `POST /assets/{id}/disposal` | `capture_asset_deletion` → `disposed_assets::dispose` → `commit_deletions` | 同上 |

本地異動失敗或回 false（404）→ 不呼叫 commit。

## 退場與清理（票 04）

- 刪除 `kea::sync` 的 `KeaSync`（移入 push）、`after_assignment_change`、`push_reservation_deletes`、`push_reservation_updates`、`push_each`、私有 `upsert`。
- 刪除 `kea/mod.rs` 的 `KeaGateway`、`Reservation`、`Lease`、`KeaError::NotImplemented`（全 repo 0 引用）並更新模組文件。
- 前端、`agent/`、完整同步邏輯不動。

## 測試決策

- 保留全部 29 個 HTTP 整合測試（`backend/tests/kea_sync.rs`，含 `StubKea`）當回歸——它們釘住 HTTP 契約、config-write 次數與前端形狀。
- 新增 `push.rs` module 測試（`#[cfg(test)]`，in-memory SQLite＋migrations，`FakePush` 記命令序列、可注入失敗）：
  - 指派：受管＋保留→一次 upsert（欄位正確）；轉 static／取消→一次 delete；未受管／無 client／兩態皆非保留→零命令、`None`；介面無 MAC→failed、無 upsert；Kea 失敗→failed 且不影響本地。
  - 刪除：跨多網段逐筆刪除、順序＝輸入序；未受管跳過；單筆失敗續行。
  - MAC：變更→逐筆 upsert（新 MAC）；未變→`None`、零命令；無保留→`None`。
- 不新增 HTTP 測試。

## 損壞資料路徑（允許差異）

- 無。本卡不改任何讀取解析；`KeaError` 訊息沿用。

## 明確不做

- 完整同步（計畫／套用）、網段層設定、`config-write` 批次化、重試佇列、同步狀態落庫。
- route 的 404／出借中／驗證邏輯搬移；`AppState` 變更；公開 port 注入。
- `ApiError::not_implemented` 等其他無關死碼（不在本卡）。

## 實作記錄

已實作（2026-10-19，票 01–04 全數完成）。

- 設計會談產出：`992782f`（spec 與票單）。實作 commits：票 01 `8343625`、票 02 `ab47505`、票 03 `a7c6b21`、票 04 `1b32cae`（各票補記見票檔 `## Comments`）。
- 終態：即時推送單一 module（`kea::push`）：4 個 capture＋3 個 commit＋內部 `KeaPush` port（正式 `HttpPush`／測試 `FakePush`）；route 只做 capture→本地異動→commit；`kea/sync.rs` 只留完整同步；五個 route 不再有任何保留查詢或推送判斷。
- 驗收：`cargo test` 479 passed／0 failed／6 ignored（`kea_connectivity` 需真機 Kea，既有）；`cargo fmt --check` 綠；`cargo check --all-targets` 無警告；`backend/src` 對 `KeaGateway`／`after_assignment_change`／`push_reservation_*`／`push_each`／`NotImplemented` 零殘留（各端點 JSON 形狀不變）。
- 與規格差異：
  1. internal commit 的 port 參數為泛型 `Option<&P> where P: KeaPush + ?Sized`（async fn trait 無 dyn 相容性，E0038）；公開函式與測試注入用法不變。
  2. `commit_interface_change` 對同一介面只重讀一次並共用其 MAC（舊碼逐筆重讀，等價）；MAC 比較基準為本地異動前的起始讀取（spec 決策 2）。
  3. `api/ips.rs` DELETE 的 `subnets::get` 保留為純 404 檢查（不再綁定 subnet 變數）。
  4. 票 04 順帶修正 `push.rs` 兩處因退場而失效的註解（零行為）。
- 後續（未做，屬另案）：架構審視報告候選 3–6（前端指派流程、匯入管線、IP 清單投影、領域原始值）。
