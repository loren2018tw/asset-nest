# 24: 側欄切換網段後 IP 頁未重載（同路由參數變更）

**What to build:** 修正票 23 的側欄網段快速入口：在 `/subnets/:id/ips` 點另一個網段項目時，URL 變了但頁面內容不變（仍顯示前一個網段）。改為監看路由參數，換網段時重載網段資訊與 IP 清單。

**Blocked by:** None (can start immediately)

**Status:** done

- [x] `IpListPage.vue` 的 `subnetId` 改為 computed（隨 `route.params.id` 更新）
- [x] `watch(() => route.params.id)`：換網段時清空清單、重設篩選與分頁後重載；離開網段頁（無 id）不觸發
- [x] 載入流程抽出 `loadSubnet()`（原 `onMounted` 內容），供首次載入與換網段共用
- [x] 過期回應守門：`subnetLoadToken`／`ipsLoadToken`，快速連點不會被較晚回來的舊請求覆蓋
- [x] `pnpm --filter frontend typecheck`、`pnpm --filter frontend lint:check`、`pnpm --filter frontend build` 全綠

## Comments

問題原因：`/subnets/1/ips` 與 `/subnets/2/ips` 是同一條路由，Vue Router（v5.3.1）的 `RouterView` 對同一 route record 只換 params 時會**重用元件實例**、不重新執行 `onMounted`；而 `subnetId` 是 setup 時 `Number(route.params.id)` 的固定值，因此頁面持續顯示舊網段（側欄高亮已換，內容沒換）。此前只能由 `/ips` 網段列表進入，從未直接「網段頁 → 網段頁」，故未暴露。

修正（僅 `frontend/src/pages/IpListPage.vue`）：

- `const subnetId = computed(() => Number(route.params.id))`；script 內改用 `subnetId.value`。
- 新增 `loadSubnet()`：`fetchSubnet` + `fetchIps`；失敗清空清單與提示。
- `watch(() => route.params.id)`：`oldId !== id` 且新 id 為正整數時，`subnet.value = null`、清空 `ips`、篩選與分頁回預設，再 `loadSubnet()`；導覽離開時（id 變 `undefined`）略過。
- 加 `subnetLoadToken`／`ipsLoadToken`（比照 `AssetFormDialog` 的載入 token 模式）：快速連點多個網段時，只有最後一次請求的結果會寫入畫面。
- 驗收：`pnpm --filter frontend typecheck`、`pnpm --filter frontend lint:check`、`pnpm --filter frontend build` 全綠。