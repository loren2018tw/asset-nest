# Kea 同步完整性：刪除、MAC 編輯與匯入提示

- 狀態：已定案（2026-10-14 grilling 兩輪逐題確認，全數採建議）
- 詞彙：`GLOSSARY.md`「同步詞彙」；決策見 `docs/adr/0027`（延伸 ADR-0011；ADR-0013、0020、0023 不變）

## 背景

盤點全專案後確認的同步缺口（現行即時推送只涵蓋指派端點的保留異動與報廢刪除）：

1. 資產刪除、介面刪除、草稿取消的「刪除已建立內容」→ 本地 CASCADE 刪除保留，Kea 保留殘留（舊 MAC 仍能拿到該 IP）。
2. 介面 MAC 編輯 → Kea 保留的 `hw-address` 過期；完整同步雖可收拾，但期間配發行為錯誤。
3. 資產 CSV 匯入 → 建立保留但零 Kea 呼叫，且完成頁無提示。
4. 網段層：刪除受管網段、清空／改號 `kea_subnet_id`、變更 CIDR 皆無 Kea 動作與警示（僅增不刪的既定語意）。
5. 錯誤路徑：報廢「先推後搬」造成反向漂移；資產端指派端點在 warnings 失敗時不推送、不回報 `kea_sync`。

## 範圍

- 即時推送擴及：資產刪除、介面刪除、「刪除已建立內容」、介面 MAC 編輯（僅 v4 受管網段、保留用途，沿用 ADR-0011 觸發條件）。
- CSV 匯入：不推送；回應帶受管網段保留數；前端條件提示執行完整同步。
- 網段層語意不變（只在完整同步推送、僅增不刪）；新增 UI 警示。
- 失敗策略不變（失敗僅警示、不阻擋本地、完整同步修復）；順修兩條錯誤路徑。

非目標：Kea 端殘留清理（`subnet4-del`）、CIDR 變更的 Kea 端自動修正、自動重試／持久同步狀態、排除範圍推送 Kea（ADR-0020 不變）、IPv6。

## 決策（grilling 定案）

| # | 決策 |
|---|------|
| Q1 | 刪除家族（`DELETE /assets/{id}`、`DELETE /interfaces/{id}`、草稿取消「刪除已建立內容」）補推送：**先本地刪除、後逐筆刪 Kea 保留**；失敗警示不阻擋。 |
| Q2 | 介面 MAC 編輯補推送：該介面在受管網段的保留指派逐筆 upsert（新 MAC）。 |
| Q6 | MAC 清空（Some→None）時若介面有保留指派 → **結構阻擋**（400，ADR-0006）；與「無 MAC 不可有保留」一致。 |
| Q7 | 回應契約：兩個 DELETE 由 204 改 200＋`kea_sync: Vec<KeaSync>`；`PATCH /interfaces/{id}` 加 `kea_sync`（僅確有推送時出現）；前端（含刪除已建立內容）失敗以 `notifyKeaSync` 警示。 |
| Q8 | 匯入不推送：`ImportReport.created` 加 `managed_reservations`（本次建立於受管網段的保留數）；前端步驟③ `>0` 時提示「請執行 Kea 完整同步」。 |
| Q9 | 網段警示：(a) 刪除確認框——受管網段加註「Kea 端網段與位址池不會被移除，需自行清理」；(b) 編輯受管網段時若 CIDR 變更或 `kea_subnet_id` 清空／改號 → 存檔前二次確認說明後果。 |
| Q5 | 失敗原則維持 ADR-0011；順修：報廢改「先搬移、後推送」；`PUT /assets/{id}/assignments` 推送移至 warnings 之前。 |
| Q10 | 記錄：新增 ADR-0027；ADR-0011／0026 補「後續修訂」指標；`GLOSSARY.md`「推送」定義修訂。 |

## 後端（票 01、02）

- `assignments::list_reservations_for_interfaces(pool, ids) -> Vec<Assignment>`：取多介面的保留指派（單一查詢）。
- `kea::sync::push_reservation_deletes(pool, client, &[Assignment])` 與 `push_reservation_updates(...)`：批次推送（網段快取；刪除＝`after=None`、更新＝`after=Some` 以最新 MAC upsert）；回傳僅含實際涉同步者。
- `api/assets.rs` `delete_asset`：讀取介面→保留（刪除前）→ 本地刪除 → 推送 → 200 `{ kea_sync }`。
- `api/interfaces.rs`：
  - `delete_interface`：同上（單一介面）。
  - `update_interface`：MAC 清空且有保留 → 400；否則 MAC 有變更時推送更新（`kea_sync` 僅非空時出現）。
  - `InterfaceResponse` 加 `kea_sync: Option<Vec<KeaSync>>`（`skip_serializing_if = "Option::is_none"`）；`InterfaceDeleted { kea_sync }`。
- `api/disposed_assets.rs`：改用共用推送；順序改「先讀保留→搬移→推送」。
- `api/asset_assignments.rs`：推送移至 `warnings_for_address` 之前。
- `import.rs`：`Created.managed_reservations`；`write_plans` 以網段快取（`SELECT kea_subnet_id FROM subnets`）計數。
- 錯誤路徑：Kea 失敗一律 `KeaSync::failed`、不阻擋本地（ADR-0011）。

## 前端（票 03、04）

- `api/assets.ts`：`AssetDeleted { kea_sync }`；`deleteAsset` 回傳之；`ImportCreated.managed_reservations`。
- `api/interfaces.ts`：`InterfaceSaved.kea_sync?`、`InterfaceDeleted`；`deleteInterface` 回傳之。
- `AssetFormDialog.vue`：`syncInterfaces` 收集 `kea_sync` 失敗並警示（介面刪除／MAC 編輯）；`deleteCreatedAsset` 警示。
- `AssetsPage.vue`：刪除後失敗警示。
- `AssetImportDialog.vue`：步驟③ `managed_reservations > 0` 提示完整同步。
- `SubnetsPage.vue`：刪除前載入網段詳情；受管網段加註 Kea 殘留警示。
- `SubnetFormDialog.vue`：載入時留存原 CIDR／`kea_subnet_id`；存檔前符合條件時 `$q.dialog` 二次確認。

## 測試

- 後端（stub Kea，沿用 `tests/kea_sync.rs` 模式）：資產／介面刪除推送（成功、失敗仍本地刪除）、MAC 編輯 upsert、MAC 清空阻擋與允許、未受管不推送（`kea_sync: []`）。
- 既有測試更新：資產／介面刪除 204→200 與 `{ kea_sync: [] }`；匯入 `managed_reservations`。
- 前端：typecheck／lint／build（無測試框架）。

## 指令

```sh
cargo test --manifest-path backend/Cargo.toml
cargo fmt --check --manifest-path backend/Cargo.toml
pnpm lint:check
pnpm --filter frontend typecheck
pnpm build:frontend
```

## 實作記錄

- 票 01、02（後端）：主實作 commit `393bcda`（`assignments::list_reservations_for_interfaces`；`kea::sync::push_reservation_deletes`／`push_reservation_updates`；資產／介面刪除與 MAC 編輯推送、MAC 清空阻擋、報廢與資產端指派順序修正；`import::Created.managed_reservations`；stub 整合測試與既有 204→200 更新）。
- 票 03、04、05（前端與文件）：主實作 commit `f541296`（`AssetDeleted`／`InterfaceDeleted`／`InterfaceSaved.kea_sync`；刪除與 MAC 推送失敗警示；匯入完成頁提示；受管網段刪除與編輯警示；README／getting-started）。
- 決策文件：`9c57558`（ADR-0027、ADR-0011／0026 後續修訂註記、GLOSSARY「推送」）。
- 驗收：`cargo test`（31 組、195 單元、0 failed、6 ignored）、`cargo fmt --check`、`pnpm lint:check`、`pnpm --filter frontend typecheck`、`pnpm build:frontend` 全綠。
