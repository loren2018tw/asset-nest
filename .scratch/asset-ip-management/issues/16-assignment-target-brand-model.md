# 16: IP 清單指派對象顯示「描述(廠牌 型號)」

**What to build:** IP 管理清單的「指派對象」第一行改為顯示「資產描述(廠牌 型號)」（廠牌／型號缺者省略，皆缺僅顯示描述），讓同名或同類資產可辨識。

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

- [ ] 後端指派對象（IP 清單列）補 `asset_brand`／`asset_model` 欄位（比照票 15 的 `asset_property_no` 做法；查詢帶出 assets 欄位）
- [ ] 第一行格式：`描述(廠牌 型號)`；只有廠牌→`描述(廠牌)`、只有型號→`描述(型號)`、皆缺→僅 `描述`
- [ ] 第二行維持介面名稱／MAC（＋hostname）；v4／v6 一致
- [ ] 後端整合測試涵蓋各缺值組合；前端 typecheck／lint 全綠
