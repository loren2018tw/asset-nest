# 07: 衝突標記與篩選

**What to build:** IP 清單出現兩個新標記——已指派位址的宣告 MAC 與觀測 MAC 不符（`ObservedMacMismatch`）、未指派且非池內位址有主（`ObservedOnUnassigned`）；並可篩選「未指派但有主」「有未登錄 MAC」。池內位址被 DHCP 正常使用時不標記。

**Blocked by:** 02 快速掃描與「最後可見」欄（核心）

**Status:** ready-for-agent

- [ ] 兩標記即時計算（不落地），以 IP 列徽章與說明呈現
- [ ] 宣告介面無 MAC 時不比對；池內位址不標 `ObservedOnUnassigned`
- [ ] 篩選為伺服器端（`unassigned_seen`、`unknown_mac`；MAC 比對不分大小寫）
- [ ] 測試與 `pnpm lint:check`／`pnpm typecheck` 全綠
