# 04: 資產匯出

**What to build:** 資產清單可將目前搜尋／篩選／排序結果輸出成與匯入範本相同的 14 欄 CSV。

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

- [ ] `GET /assets/export` 同清單查詢參數（q／location／brand／device_serial／tag／sort／dir；忽略分頁）；回 `text/csv`（attachment）
- [ ] 14 欄與 `docs/adr/0008` 一致（標籤以 `|` 串接、日期 `YYYY-MM-DD`、MAC 小寫冒號）；一列一資產
- [ ] 介面與位址選取規則依 spec：有指派位址的介面中 id 最小者；v4／v6 各取該介面數值最小者；hostname 僅當匯出的 IPv4 為 reservation（其餘空）
- [ ] 資產工具列新增「匯出」按鈕（檔名 `資產匯出_YYYYMMDD.csv`）；UTF-8 BOM
- [ ] 後端整合測試涵蓋：篩選條件套用、多介面／多 IP 選取規則、無介面、標籤與日期格式
