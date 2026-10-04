# 13: IP 清單「位置」欄與位置搜尋

**What to build:** 網段的 IP 管理清單新增獨立「位置」欄（顯示指派對象所屬資產的位置），並讓關鍵字搜尋涵蓋位置。

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

- [ ] 新增「位置」欄，位於「狀態／用途」與「指派對象」之間；有指派顯示該資產位置，未指派顯示「—」
- [ ] 「指派對象」不再重複顯示位置（顯示資產描述、介面名稱／MAC，保留 hostname）
- [ ] 關鍵字比對納入位置（子字串、大小寫無關），與 IP／資產描述／介面名稱／MAC 並列；搜尋框 placeholder 更新
- [ ] v4 與 v6 行為一致
- [ ] 後端整合測試涵蓋位置欄位與位置關鍵字（含無結果）；前端 typecheck／lint 全綠
