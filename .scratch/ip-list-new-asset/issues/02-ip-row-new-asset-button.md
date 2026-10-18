# 02: IP 清單列「新增資產及指派 IP」按鈕

**What to build:** `IpListPage` 操作欄（僅 v4 網段）新增圖示按鈕（`add_link`＋tooltip「新增資產及指派 IP」）：狀態「可用」啟用，其餘停用＋tooltip（池內→「池內位址不可指派」、排除→「排除範圍內位址不可指派」、手動／保留→「此位址已有綁訂資產」）。點擊開啟 `AssetFormDialog`（新增模式）：預填描述（空字串）、介面草稿（有 `last_seen_mac` 時 `eth0`＋MAC）、指派草稿（該列位址、用途手動設定）；儲存後重載清單、停留原頁。詳見 `.scratch/ip-list-new-asset/spec.md`。

**Blocked by:** 01

**Status:** done

- [x] 操作欄按鈕（僅 v4 網段顯示；v6 不顯示——非停用）
- [x] 啟用條件 `status === "available"`；停用三組 tooltip 文字如上（池內／排除沿用既有按鈕的文字）
- [x] 點擊預填並開啟 `AssetFormDialog`（描述空、`last_seen_mac` 條件介面草稿、指派草稿 `{ address, purpose: "static", hostname: null, subnetId, subnetLabel }`）
- [x] `@saved` 重載 IP 清單、停留原頁
- [x] README「IP 管理」描述補「可自可用位址建立資產並指派」
- [x] `pnpm --filter frontend typecheck`、`pnpm lint:check` 綠；`cargo test`（後端未動）回歸綠

## 注意

- 「手動設定（static）」與「保留（reservation）」皆代表已綁訂資產——比原始口述（僅停用池內／保留／排除）**多停用 static**，這是定案決策，不是遺漏。
- 「非法佔用 IP」的可用列按鈕照常啟用（建檔指派後衝突消除）。
- 觀測 MAC 預填落地後若出現 DuplicateHwAddress 語意警示，照 ADR-0006 不阻擋。
- 不做 MAC 反查、不做自動跳轉、不動後端。

## Comments

實作完成：

- `IpListPage.vue`：操作欄最前新增 `add_link` 圖示按鈕（外層 `<span>` 包裹，確保停用時 tooltip 仍顯示；僅 v4 顯示）；`createDisabledReason()` 四態（available→`null`、in_pool／excluded 沿用既有文字、static／reservation→「此位址已有綁訂資產」）；`createPrefillInterface`（有 `last_seen_mac` 時 `eth0`＋MAC）與 `createPrefillAssignment`（該列位址、`purpose: "static"`、`subnetId`＝路由參數、`subnetLabel`＝`subnetDisplayLabel(網段名稱, CIDR)`）兩個 computed；獨立掛載一個 create-only 的 `asset-form-dialog`（`:asset="null"`，比照 `KeaLeasesPage` 模式，不與「編輯綁訂資產」的編輯用實例混用）；`@saved` 走 `fetchIps()`（保留分頁，停留原頁）。
- 描述不預填：未傳 `prefill-description`，對話框 `prepare()` 即得空字串（列上本無 hostname）。
- 無觀測 MAC 的可用列：`prefill-interface` 為 `null`，由對話框（票 01）補一筆空 MAC 的 `eth0` 宿主列承接指派草稿。
- 驗收：`pnpm --filter frontend typecheck`、`pnpm lint:check`（73 檔）、`pnpm build:frontend` 全綠；`cargo test` 406 passed／0 failed／6 ignored（後端未變更）。
- 未驗證：瀏覽器人工檢核（停用 tooltip、預填內容、落地與 Kea 推送提示）——待有環境時執行。

後續修訂（更名，避免與「新增資產及指派 IP」混淆）：

- `IpListPage.vue`：既有指派按鈕（edit 圖示）未指派情境的 aria-label 由「指派」改為「指派 IP 給資產」，並補上可見 tooltip（未指派→「指派 IP 給資產」、已指派→「編輯指派」，後者不變）；池內／排除兩個停用變體的 aria-label 同步改為「指派 IP 給資產」（tooltip 維持停用原因）。
- `AssignmentDialog.vue`：v4 未指派情境的對話框標題由「指派 IP」改為「指派 IP 給資產」（編輯指派、v6「新增位址／編輯位址」不變）。
- 驗收：`pnpm --filter frontend typecheck`、`pnpm lint:check` 綠。
