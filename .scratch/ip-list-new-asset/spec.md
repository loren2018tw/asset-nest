# IP 清單：可用位址「新增資產及指派 IP」入口

對應票號：01（AssetFormDialog 預填指派草稿）、02（IP 清單列按鈕）。

詞彙依 `GLOSSARY.md`（Assignment、指派草稿、可用、池內、排除、Reservation）；本次補登「指派草稿（Assignment Draft）」詞條（ADR-0025 既有概念，詞典漏登）。與租約清單同名入口（`.scratch/kea-lease-to-asset/`）的差異見「背景與決策」。本功能不新增 ADR；後端完全不變。

## 目標

- IP 清單（`/subnets/:id/ips`）狀態「可用（available）」的列提供「新增資產及指派 IP」入口：開啟既有「新增資產」對話框，預填一筆「該列位址」的指派草稿（用途＝手動設定）與（有觀測 MAC 時）一筆 `eth0`＋觀測 MAC 的介面草稿；資產儲存時草稿一併落地（ADR-0025）、含既有 Kea 推送。
- 與租約清單同名按鈕不同：租約版**不指派**（租約 IP 幾乎必落池內，結構上指派不了，見 `kea-lease-to-asset/spec.md`）；IP 清單版面對「可用」位址，**真的指派該位址**——按鈕名稱在此版為如實描述。

## 背景與決策

grilling 定案的五項決策：

1. **按鈕行為＝預填＋真指派**：租約版只預填是當時的權衡（池內結構限制），非原則；IP 清單的可用位址無此限制，且「停用池內／排除」的設計唯有在「真的要指派」的前提下才成立。
2. **啟用條件＝狀態「可用」**（較原始口述擴大）：原口述停用「池內／保留／排除」；但 IP 清單狀態「手動設定（static）」與「保留（reservation）」皆代表**已綁訂資產**（`backend/src/ips.rs::status_of` 以指派用途推導），按鈕按下去只會造成重複資產與位址占用衝突，故一併停用。要改綁既有位址者，用操作欄既有的「編輯綁訂資產」「指派／編輯指派」按鈕。
3. **預填來源**：IP 列無 hostname，僅 `last_seen_mac`。有觀測 MAC 才預填介面草稿（`eth0`＋`last_seen_mac`，皆可編輯）；描述一律留空。從未上線的可用位址開空白表單（仍帶指派草稿）。「非法佔用 IP」（ObservedOnUnassigned）的可用列正是本入口的受益者：建檔指派後衝突自然消除。
4. **指派草稿用途預設＝手動設定（static）**：可用位址在 DHCP 位址池外，屬手動管理空間；草稿一經預填不可改（僅能取消後自介面草稿「指派 IP」重選），故預設取誤信度最低者。要 Kea 保留者取消草稿重選「DHCPv4 保留」。
5. **v6 網段不顯示按鈕**：v6 位址登錄即指派（用途恆 static），不存在「可用」列；一排永遠停用的按鈕是雜訊。

## 範圍

- 僅 **IPv4** 網段的 IP 清單顯示按鈕；v6 不顯示（非停用）。
- 衝突語意照舊（ADR-0006）：重複 MAC 等語意警示不阻擋；結構錯誤（如位址被搶先指派）由落地時既有驗證回報。
- 不含：MAC 反查既有資產、池內／排除規則調整、v6 支援、後端變更、自動跳轉。

## 前端

### `frontend/src/components/AssetFormDialog.vue`（票 01）

- 新增選填 prop（僅新增模式套用，編輯模式忽略）：
  `prefillAssignment?: { address: string; purpose: IpPurpose; hostname: string | null; subnetId: number | null; subnetLabel: string | null } | null`
- `prepare()` 時若提供，置入一筆 `assignmentDrafts`（結構同現行、`key` 遞增規則不變；與既有 `prefillDescription`／`prefillInterface` 可並用）。
- 草稿須綁在介面草稿列上：無預填介面時自動補一筆空 MAC 的 `eth0` 宿主列（MAC 空白＝手動設定介面，可編輯）。
- 未提供時行為完全不變；落地沿用 `preflightAssignmentDrafts()`／`landAssignmentDrafts()` 既有機制（含 Kea 推送結果呈現）。

### `frontend/src/pages/IpListPage.vue`（票 02）

- 操作欄（僅 v4 網段）新增圖示按鈕（icon `add_link`、dense、與既有三鈕同款），tooltip＝「新增資產及指派 IP」；一律顯示，不合用時 `disable`＋tooltip 說明原因：
  - `status === "in_pool"` →「池內位址不可指派」（沿用既有文字）；
  - `status === "excluded"` →「排除範圍內位址不可指派」（沿用既有文字）；
  - `status === "static" | "reservation"` →「此位址已有綁訂資產」；
  - `status === "available"` → 啟用。
- 點擊：開啟 `AssetFormDialog`（`asset = null`），預填：
  - `prefillDescription`＝空字串；
  - `prefillInterface`＝`last_seen_mac !== null` 時 `{ name: "eth0", mac: last_seen_mac }`，否則 `null`；
  - `prefillAssignment`＝`{ address: 列位址, purpose: "static", hostname: null, subnetId: 路由網段 id, subnetLabel: 頁面既有網段標籤 }`。
- 對話框 `saved` → 重載 IP 清單、停留原頁。

### `README.md`

- 「IP 管理」描述補一句：可自可用位址建立資產並指派。

## 測試

- 前端無測試基礎設施：`pnpm --filter frontend typecheck`、`pnpm lint:check` 把關；後端未變更，`cargo test` 回歸綠。
- 人工檢核（`pnpm dev`）：
  - 可用列啟用；池內／排除／手動／保留列停用＋對應 tooltip 文字正確。
  - 點擊後對話框：指派區顯示該位址草稿（用途「手動設定」、標記未儲存）；有觀測 MAC 時介面草稿 `eth0`＋MAC、無觀測 MAC 時無介面草稿；描述為空。
  - 儲存後 IP 清單重載：該列變「手動設定」、指派對象為新資產；Kea 推送結果照既有呈現；停留在原頁。
  - v6 網段頁不出現按鈕。
  - 既有入口（租約頁、資產頁、CSV 匯入）不回歸：未提供 `prefillAssignment` 時行為不變。

## 指令

```sh
pnpm --filter frontend typecheck
pnpm lint:check
cargo test --manifest-path backend/Cargo.toml   # 回歸（後端未變更）
```

## 實作記錄

（主實作 commit `28b88fb`）

- 票 01（AssetFormDialog 預填指派草稿）：實作完成。新增選填 prop `prefillAssignment`（型別沿用 `AssignmentDraftInput | null`；僅新增模式、編輯模式忽略）；草稿置入點在 `loadInterfaces()` 新增模式分支（`rowKey` 綁預填介面草稿列，與 `prefillInterface` 同處建立、不依賴 async 時序）；無預填介面時補一筆空 MAC 的 `eth0` 宿主列。
- 票 02（IP 清單列按鈕）：實作完成。`IpListPage.vue` 操作欄最前新增 `add_link` 圖示按鈕（外層 `<span>` 供停用時 tooltip 顯示；僅 v4 顯示）；`createDisabledReason()`：available→啟用、in_pool／excluded 沿用既有文字、static／reservation→「此位址已有綁訂資產」；`createPrefillInterface`／`createPrefillAssignment` computed（後者 `purpose: "static"`、`subnetLabel` 由 `subnetDisplayLabel` 組）；獨立掛載 create-only 的 `asset-form-dialog`；`@saved` 走 `fetchIps()`（保留分頁、停留原頁）。`README.md`「IP 管理」補「可自可用位址建立資產並指派」。
- 驗收：`pnpm --filter frontend typecheck`（vue-tsc）、`pnpm lint:check`（oxfmt＋oxlint、73 檔）、`pnpm build:frontend` 全綠；`cargo test --manifest-path backend/Cargo.toml` 406 passed／0 failed／6 ignored（後端未變更）。
- 未驗證：瀏覽器人工檢核（停用 tooltip 四態、預填內容、落地與 Kea 推送提示、v6 頁無按鈕）——待有環境時執行。
- 後續修訂（更名）：既有指派按鈕未指派情境更名為「指派 IP 給資產」（`IpListPage.vue` aria-label＋新增可見 tooltip；`AssignmentDialog.vue` v4 未指派的對話框標題同步），與本功能的「新增資產及指派 IP」（建新資產）區別；「編輯指派」「編輯綁訂資產」與 v6 標題不變。
