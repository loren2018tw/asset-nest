# 06: 觀測歷史對話框與匯出

**What to build:** 從 IP 清單開啟任一 IP 的「觀測歷史」：事件時間軸（首見、MAC 變更）與用過的 MAC 清單（各自首見／最後可見／來源）；可切到 MAC 視角查「用過哪些位址」；未知 MAC 標示「未登錄」、已知 MAC 可連到對應資產；可匯出單一 IP 的歷史 CSV。

**Blocked by:** 02 快速掃描與「最後可見」欄（核心）

**Status:** ready-for-agent

- [ ] IP 歷史端點：現況＋事件清單（時間新到舊）
- [ ] MAC 歷史端點：用過的位址、每筆首見／最後可見，標示已知／未知與資產連結
- [ ] CSV 匯出（`address, mac, kind, source, observed_at`；檔名沿用既有下載慣例）
- [ ] 前端歷史對話框支援「以 IP 進入」與「以 MAC 進入」；IP 列新增「觀測」操作入口
- [ ] 測試（端點與匯出格式）與 `pnpm lint:check`／`pnpm typecheck` 全綠
