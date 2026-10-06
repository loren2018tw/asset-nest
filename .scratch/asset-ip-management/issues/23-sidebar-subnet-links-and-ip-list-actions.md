# 23: 側欄網段快速入口與 IP 清單列操作調整

**What to build:** 側欄「IP 管理」下方列出已設定網段（每網段一項、連往該網段 IP 清單）；IP 清單列移除「取消指派」（只在編輯時取消），新增「編輯綁訂資產」按鈕——已指派介面時直接開啟該資產的編輯對話框，未指派介面時停用。

**Blocked by:** None (can start immediately)

**Status:** done

- [x] 側欄：`MainLayout.vue` 於「IP 管理」下方以 `listSubnets()` 列出網段（有名稱顯示名稱＋CIDR 副標，無名稱顯示 CIDR），連往 `/subnets/{id}/ips`；所在網段項目高亮；換頁時重載
- [x] IP 清單列：移除「取消指派」按鈕與其確認／呼叫流程（`cancelAssignment`、`cancelAssignmentHint`、`notifyKeaSync` 不再由此頁使用）
- [x] IP 清單列：新增「編輯綁訂資產」（icon `inventory_2`）；已指派列以 `fetchAsset(asset_id)` 載入後開啟 `AssetFormDialog`，儲存後重載清單；未指派介面時停用並以 tooltip 說明
- [x] 取消指派仍可於編輯指派對話框（既有 `AssignmentDialog` 編輯模式）與資產編輯對話框的已指派 IP 清單進行
- [x] `pnpm --filter frontend typecheck`、`pnpm --filter frontend lint:check`、`pnpm --filter frontend build` 全綠

## Comments

實作完成（未動後端；沿用既有 `GET /assets/{id}`、`GET /subnets`）。

- 側欄（`frontend/src/layouts/MainLayout.vue`）：
  - 新增 `subnets`（`SubnetSummary[]`）與 `loadSubnets()`；`onMounted` 及 `watch(route.path)` 時載入，讓新增／刪除／改名後換頁即反映（載入失敗僅略過，不干擾頁面）。
  - 「IP 管理」改為僅 `/ips` 高亮（`exact`）；下方以 `v-for` 產生網段項目（`dense`、icon `format_list_numbered`、label `ellipsis`），以 `activeSubnetId`（自 `route.params.id` 解析）個別高亮。
- IP 清單（`frontend/src/pages/IpListPage.vue`）：
  - 列操作依序為：觀測歷史、編輯綁訂資產、編輯指派（池內未指派列維持停用編輯）。
  - `openBoundAsset()`：以列上 `assignment.asset_id` 呼叫 `fetchAsset` 取得詳情（`AssetDetail extends Asset`）後開啟 `AssetFormDialog`；讀取中該列按鈕轉 loading，同時間僅允許一列（`assetLoadingAddress` 守門）；失敗 notify。
  - `@saved` 一律 `fetchIps()`：資產描述／位置變更即時反映，於對話框內取消指派亦同步。
  - 移除列上「取消指派」及其確認對話框、`cancel()` 與相關 import；`AssignmentDialog` 編輯模式的「取消指派」與資產編輯對話框清單入口不變（見票 20）。
- 決策／取捨：
  - 未指派介面的列按鈕保留但停用（與池內列停用編輯一致），tooltip 顯示「此位址未指派介面」，而非隱藏按鈕。
  - 側欄清單於換頁時重載（`route.path`），不引入 store／事件匯流排。
- 規格：`spec.md` 追加定案（2026-10-06）；§4.3、§4.4 同步。
- 驗收：`pnpm --filter frontend typecheck`、`pnpm --filter frontend lint:check`、`pnpm --filter frontend build` 全綠。