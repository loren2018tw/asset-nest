# 17: 移除 IP 清單 Gateway 欄位與排序

**What to build:** IP 管理清單不再顯示 Gateway 欄位、也不提供以此排序；gateway 僅在網段設定的編輯對話框可見。

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

- [ ] 前端移除 Gateway 欄位與其 cell template
- [ ] 後端排序白名單移除 `gateway`（`sort=gateway` 回 400）
- [ ] 回應不再包含 `is_gateway` 欄位（後端型別、列建構、前端型別與相關測試同步移除；清除因此不再使用的程式碼）
- [ ] 票 14 的 Gateway 排序測試與單元測試相應調整；既有行為回歸全綠
- [ ] 前端 typecheck／lint 全綠
