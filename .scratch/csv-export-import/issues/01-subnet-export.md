# 01: 網段匯出

**What to build:** 網段設定頁可將全部網段設定輸出成 CSV（格式依 `docs/adr/0009`），供空庫重建時匯入。

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

- [ ] `GET /subnets/export` 回 `text/csv`（attachment）：欄位＝名稱／CIDR／Gateway／Kea subnet-id／位址池／備註；僅 v4 有 Kea subnet-id 與 pool（多段以 `|` 分隔、每段 `起點-終點`）
- [ ] 排序 v4 先、v6 後，同族依 CIDR 數值；UTF-8 BOM；第一列標題列
- [ ] 網段設定工具列新增「匯出」按鈕，點擊即下載（檔名 `網段匯出_YYYYMMDD.csv`）
- [ ] 後端整合測試涵蓋：v4 多 pool、v6（兩欄留空）、選填欄位缺值、BOM 與標題列
