# 05: 探索掃描與未知設備

**What to build:** 管理者可在網段開啟探索掃描並設定間隔（預設每日，可用 `OBSERVATION_DISCOVERY_INTERVAL_SECS` 調整）；探索對全網段 host 位址限速探測（`OBSERVATION_DISCOVERY_RATE_PPS`），發現未指派卻有主的位址與未登錄 MAC；表單顯示上次探索時間；可手動立即探索。

**Blocked by:** 04 背景排程與保留清理

**Status:** ready-for-agent

- [ ] 網段 PATCH 支援 `discovery_enabled`（需先開觀測，否則 400）與探索間隔（正整數或空＝全站預設）
- [ ] 探索目標＝該網段全部 host 位址（測試以 stub 斷言），依速率限速發送；更新 `last_discovery_at`
- [ ] 未指派被看見者建立現況列、未看見者不建列；事件規則同快速掃描
- [ ] 排程依網段間隔觸發探索；`POST /subnets/{id}/sweeps` 支援 mode=discovery
- [ ] 網段表單探索區塊（開關／間隔／上次探索／立即探索）；`.env.example` 更新
- [ ] 測試與 `pnpm lint:check`／`pnpm typecheck` 全綠
