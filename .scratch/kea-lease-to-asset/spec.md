# Kea 租約清單：新增資產入口、保留篩選與到期時間排序

對應票號：01（租約列入口與保留篩選）、02（到期時間排序）。

詞彙依 `GLOSSARY.md`（Lease、保留（reserved）、Assignment）；無新增詞條。與 ADR-0006 的對齊註記見其 2026-10-08 修訂；本功能不新增 ADR。

## 目標

- 租約清單（`/kea/leases`）可依「保留位址／非保留位址」篩選（對應 `is_reservation`）。
- 「到期時間」欄可排序；預設以到期時間遞減（越晚到期越上面）。
- 非保留租約列提供「新增資產及指派 IP」入口：開啟既有「新增資產」對話框，預填資產描述（租約 hostname）與一筆網路介面（`eth0`＋租約 MAC）。
- 指派沿用既有資產編輯情境：由使用者在介面列「指派 IP」（或 IP 管理）自行輸入位址；本功能**不自動指派、不預填 IP、不自動跳轉**。

## 背景與決策

- 原需求「新增資產及指派 IP」經確認：指派本身就是編輯資產時的使用者動作，故入口只負責開好新增資產對話框並預填，其餘沿用既有流程。
- 動態租約 IP 幾乎必落在 DHCP 位址池內；「池內位址不可指派」**維持結構阻擋**（評估過的「放行池內保留」不採）。要保留租約 IP 者，需先於網段設定把該位址移出 DHCP 位址池，再以既有指派流程設為保留。
- 租約資料維持唯讀：本功能不呼叫 Kea 寫入、不影響同步；後端完全不變。

## 範圍

- 僅**受管網段**（`subnet_cidr` 非 null）租約列提供入口；未受管（如 `Kea #id`）與缺 IP 的列停用（仍顯示按鈕）。
- 不偵測 MAC 是否已登錄於其他介面：沿用既有「全系統重複 MAC」儲存警示。
- 狀態不限（declined／released／expired-reclaimed 亦是）：只要 IP 與 MAC 具備即可建檔。
- 不含：自動指派、指派對話框預填 IP、MAC 反查既有資產、池內規則調整。

## 前端

### `frontend/src/pages/KeaLeasesPage.vue`

- 篩選列（與「狀態」並列）新增 `q-select`：label「保留」、clearable、選項「保留位址」（`is_reservation === true`）／「非保留位址」（`false`）；與搜尋、狀態以 AND 疊加，客戶端過濾。
- 表格尾欄「操作」，每列 `q-btn`（dense、outline、primary，label「新增資產及指派 IP」）：
  - 一律顯示；不合用時 `disable` ＋ tooltip：
    - `is_reservation === true` →「此位址已是保留（已有資產設定）」；
    - `subnet_cidr === null` →「租約網段未受管，不提供資產建檔」；
    - `ip_address === null` →「租約缺少 IP，不提供資產建檔」；
    - `hw_address === null` →「租約缺少 MAC，不提供資產建檔」（防禦性；實務不發生）。
  - 點擊：開啟 `AssetFormDialog`（`asset = null`），預填描述＝租約 hostname（trim；空為空字串）與一筆介面草稿（name「eth0」、mac＝租約 `hw_address`；皆可編輯）。
  - 對話框 `saved` → 重載租約清單、停留原頁（無 Kea 推送，無同步通知）。

### 到期時間排序（票 02）

- `expires_at` 欄 `sortable: true`；`pagination` 預設 `sortBy: "expires_at"`、`descending: true`（每頁 50 不變）。
- 以自訂 `:sort-method` 統一排序（欄位自身的 `sort` 不再使用；不依賴 Quasar 內建 null 擺位）：
  - 到期時間以時間戳比較（顯示仍為本地時區）；
  - `null` 或無法解析的到期時間固定排最後（升／降冪皆同；沿用全站「空白固定最後」慣例）；
  - 同值以 IP 數值升冪決勝；
  - `ip_address` 欄排序維持可用（數值比較；`null`／無效值同樣固定最後）。

### `frontend/src/components/AssetFormDialog.vue`

- 新增選填 props（僅於新增模式套用）：`prefillDescription?: string`、`prefillInterface?: { name: string; mac: string } | null`；`prepare()` 時填入表單與介面草稿（草稿結構同現行，`id = null`）。
- 未提供 props 時行為完全不變；編輯模式不受影響。

### `README.md`

- 租約清單描述由「唯讀」微調為可作為資產建檔入口（唯讀檢視＋自租約新增資產）。

## 測試

- 前端無測試基礎設施：`pnpm --filter frontend typecheck`、`pnpm lint:check` 把關。
- 人工檢核（`pnpm dev`）：
  - 「保留」篩選與狀態、搜尋 AND 疊加正確；清除後還原全部。
  - 保留列按鈕停用＋tooltip；未受管／缺 IP 列停用；可用列開啟對話框且描述、eth0、MAC 已預填。
  - 儲存後租約頁自動重載；由資產編輯的介面列「指派 IP」照常運作（含池內被擋的既有提示）。
  - 進頁預設「越晚到期越上面」；點「到期時間」欄頭切換升降冪；無到期時間（—）固定最後；IP 欄排序仍可用。
- 後端未變更：`cargo test --manifest-path backend/Cargo.toml` 回歸綠。

## 指令

```sh
pnpm --filter frontend typecheck
pnpm lint:check
cargo test --manifest-path backend/Cargo.toml   # 回歸（後端未變更）
```

## 實作記錄

- 票 01（租約列入口與保留篩選）：實作完成（主實作 commit `c80950f`）。`KeaLeasesPage.vue` 篩選「保留」（保留位址／非保留位址）、操作欄「新增資產及指派 IP」按鈕（停用四條件＋tooltip）、`AssetFormDialog` 預填（描述＝hostname、eth0＋租約 MAC）、`@saved` 重載清單；`AssetFormDialog.vue` 新增選填 props `prefillDescription`／`prefillInterface`（僅新增模式）；`README.md` 租約清單描述微調。
- 票 02（到期時間排序）：實作完成（主實作 commit `c80950f`）。`expires_at` 可排序；`pagination` 預設 `sortBy: "expires_at"`、`descending: true`；新增 `:sort-method="sortLeases"` 統一排序（null／無效值固定最後、同值以 IP 數值升冪決勝；`ip_address` 集中同函式）。
- 驗收：`pnpm --filter frontend typecheck`（vue-tsc）、`pnpm lint:check`（oxfmt＋oxlint、65 檔）、`pnpm build:frontend` 全綠；`cargo test --manifest-path backend/Cargo.toml` 406 passed／0 failed／6 ignored（後端未變更）。
- 未驗證：瀏覽器人工檢核（篩選疊加、預填內容、儲存後重載、預設排序與欄頭切換）——待有環境時執行。
