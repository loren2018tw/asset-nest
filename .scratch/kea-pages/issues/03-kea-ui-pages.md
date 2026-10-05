# 03: 前端 Kea 區段（租約清單與系統狀態）

**What to build:** 側邊欄新增「Kea」區段（`q-item-label`＋兩個項目）與路由 `/kea/leases`、`/kea/status`；`api/kea.ts` 新增 `getKeaStatus()`／`listKeaLeases()` 與型別；租約清單頁（client-side 表格：IP／MAC／Hostname／網段／到期／狀態 chip、搜尋＋狀態篩選、IP 數值排序、每頁 50、已到期淡化、錯誤 banner＋前往狀態連結）；系統狀態頁（連線／版本／監聽介面／運行資訊／DHCPv4 摘要五區塊、分區錯誤、未監聽文案、網段數不一致 warning）；README 補功能介紹。詳見 `.scratch/kea-pages/spec.md`。

**Blocked by:** 01, 02

**Status:** ready-for-agent

- [ ] 選單區段與 `/kea` active 高亮；兩路由 lazy import
- [ ] 租約頁：欄位與狀態 chip（中文標籤＋原文 tooltip）、已到期淡化、搜尋／篩選、每頁 50、重新整理＋上次更新時間
- [ ] 未設定／連線失敗 banner＋「前往系統狀態」連結，空表
- [ ] 狀態頁五區塊；空 interfaces 文案「未監聽任何介面（不主動服務 DHCP；安裝預設）」；sockets 有值時並列「實際綁定」；網段數不一致 warning chip
- [ ] `pnpm lint:check` 綠；README 補新頁面介紹

## Comments
