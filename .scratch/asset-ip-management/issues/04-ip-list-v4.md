# 04: v4 IP 清單(唯讀)

**What to build:** 自網段列表進入該網段的 IP 管理頁;v4 位址全枚舉、狀態推導與瀏覽(尚無指派功能)。

**Blocked by:** 03 網段設定

**Status:** ready-for-agent

- [ ] v4 網段列出全部 host 位址:扣除 network/broadcast,`/31`、`/32` 全數列出;可為任意前綴
- [ ] IP 以數值排序;伺服器端分頁(預設 50 筆/頁)與 IP 關鍵字搜尋
- [ ] 落在 pool 範圍的列標示「池內」,且無編輯入口;gateway 位址顯示標記
- [ ] 未被指派且不在 pool 內的列顯示「可用」
- [ ] 後端整合測試涵蓋枚舉邊界(/31、/32、一般前綴)與 pool/gateway 標示
