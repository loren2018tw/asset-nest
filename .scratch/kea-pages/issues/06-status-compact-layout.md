# 06: 狀態頁運行資訊與 DHCPv4 摘要改多欄排列

**What to build:** 「系統狀態」頁的「運行資訊」與「DHCPv4 摘要」不再一項一列，改以網格排列（運行資訊 2 欄；DHCPv4 摘要桌機 3 欄、窄螢幕 2 欄），降低卡片縱向佔用。

**Blocked by:** None (can start immediately)

**Status:** done

- [x] 運行資訊：PID／運行時間／上次設定重載／socket 狀態以 `col-6` 兩欄網格呈現（4 項成 2×2）
- [x] DHCPv4 摘要：Kea 網段數／本地受管網段數／租約庫類型以 `col-6 col-md-4` 呈現；不一致 chip 與 tooltip 保留
- [x] 分區錯誤與「無法取得」呈現不變；typecheck／lint／build 全綠

## Comments

實作完成（2026-10-05）。

- `KeaStatusPage.vue`：兩區塊由 `q-list`／`q-item` 改為 `row q-col-gutter-md` 網格（標籤沿用 `text-caption text-grey-7` 樣式，與「asset-nest 服務」區塊一致）；`subnetCountMismatch` 的「不一致」chip 與 tooltip 保留於「Kea 網段數」欄位。
- 欄寬：運行資訊卡片為半寬（`col-md-6`）故固定 2 欄；DHCPv4 摘要為全寬，`col-md-4` 於桌機 3 欄、窄螢幕 2 欄。
- 驗收：`pnpm --filter frontend typecheck`、`pnpm lint:check`、`pnpm build:frontend` 全綠。
