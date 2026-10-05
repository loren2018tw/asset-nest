# 03: 前端 Kea 區段（租約清單與系統狀態）

**What to build:** 側邊欄新增「Kea」區段（`q-item-label`＋兩個項目）與路由 `/kea/leases`、`/kea/status`；`api/kea.ts` 新增 `getKeaStatus()`／`listKeaLeases()` 與型別；租約清單頁（client-side 表格：IP／MAC／Hostname／網段／到期／狀態 chip、搜尋＋狀態篩選、IP 數值排序、每頁 50、已到期淡化、錯誤 banner＋前往狀態連結）；系統狀態頁（連線／版本／監聽介面／運行資訊／DHCPv4 摘要五區塊、分區錯誤、未監聽文案、網段數不一致 warning）；README 補功能介紹。詳見 `.scratch/kea-pages/spec.md`。

**Blocked by:** 01, 02

**Status:** done

- [x] 選單區段與 `/kea` active 高亮；兩路由 lazy import
- [x] 租約頁：欄位與狀態 chip（中文標籤＋原文 tooltip）、已到期淡化、搜尋／篩選、每頁 50、重新整理＋上次更新時間
- [x] 未設定／連線失敗 banner＋「前往系統狀態」連結，空表
- [x] 狀態頁五區塊；空 interfaces 文案「未監聽任何介面（不主動服務 DHCP；安裝預設）」；sockets 有值時顯示 socket 狀態（真機為物件 `{"status":"ready"}`，非綁定清單）；網段數不一致 warning chip
- [x] `pnpm lint:check` 綠；README 補新頁面介紹

## Comments

實作完成（commit `a6362b8`，`03 Kea 檢視：前端 Kea 區段（選單／路由／API／租約與狀態頁）`）。

- `MainLayout.vue`：新增 `q-item-label` 標題「Kea」與「租約清單」（icon `receipt_long`）、「系統狀態」（icon `monitor_heart`）兩項目；各項目以 `/kea/leases`、`/kea/status` 前綴判定 `:active`（比照 `ipSection`，各自頁面保持高亮）。
- `routes.ts`：新增 `kea/leases`、`kea/status` 兩 lazy routes。
- `api/kea.ts`：新增 `KeaStatus`／`KeaVersionBlock`／`KeaSocketStatus`／`KeaRuntimeInfo`／`KeaDhcp4Block`／`KeaStatusErrors`／`KeaLease` 型別與 `getKeaStatus()`、`listKeaLeases()`（後者解開 `{leases}` 回陣列；沿用 `apiGet`）。
- `KeaLeasesPage.vue`：client-side `q-table`（每頁 50）——IP／MAC／Hostname／網段（`CIDR（名稱）`、無對應 `Kea #id`）／到期時間（本地時區；已過淡化）／狀態 chip（`default` 使用中、`declined` 已拒絕、`expired` 已過期、`released` 已釋放；未知顯示原值；tooltip 顯示原始字串）；預設 IP 升冪以八位元組數值比較（非字串序）；搜尋（IP／MAC／hostname、不分大小寫）＋狀態篩選；進頁自動載入、「重新整理」＋上次更新時間；載入失敗（400／502 等）頂部 `q-banner` 顯示後端 message＋「前往系統狀態」連結並清空表格。
- `KeaStatusPage.vue`：五區塊——連線（可達／未設定「未設定 Kea 連線（KEA_API_URL）」／失敗；顯示 `url` 與 version 錯誤訊息）、版本（`version ?? text`）、監聽介面（設定值；空＝「未監聽任何介面（不主動服務 DHCP；安裝預設）」）、運行資訊（pid／uptime 人化如「2 天 3 小時」／reload 人化「X 前」如「4 小時前」／socket 狀態一行如「socket 狀態：ready」；依票 01 真機實測不做綁定清單）、DHCPv4 摘要（Kea 網段數 vs 本地受管網段數，不一致加 warning chip＋tooltip；租約庫類型）；分區失敗顯示「無法取得」＋該區錯誤訊息；進頁自動載入、「重新整理」＋上次更新時間。
- `README.md`：首段移除已過時的「目前僅骨架」，補 Kea 區段（租約清單／系統狀態）功能介紹，統一「介面」用語。
- spec 前端段依票 01／02 真機實測以最小幅度調整：移除「有 runtime sockets 時並列實際綁定」、reload 由「本地時間」改「人化 X 前」、補「版本顯示 `version ?? text`」與 socket 狀態呈現、tooltip 字句修正。
- 驗收：`pnpm lint:check` 綠（oxfmt＋oxlint）；`pnpm --filter frontend typecheck`（vue-tsc）綠；`pnpm build:frontend`（quasar build）成功（產出 `KeaLeasesPage`／`KeaStatusPage` chunk）。
- 實作 commit：`a6362b8`。
