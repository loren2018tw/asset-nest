# 04: 背景排程與保留清理

**What to build:** 開啟觀測的網段自動每 15 分鐘快速掃描，不需手動；事件依保留期（預設 365 天，可用 `OBSERVATION_RETENTION_DAYS` 調整）每日自動清理且不影響現況；README 補上新設定與功能說明。

**Blocked by:** 02 快速掃描與「最後可見」欄（核心）

**Status:** ready-for-agent

- [ ] 排程器每 60 秒 tick；快速掃描每網段 15 分鐘；同時僅執行一個掃描；啟動後無上次紀錄者視為 due
- [ ] 保留清理只刪過期事件、不動現況；以固定 `now` 可測
- [ ] 環境變數 `OBSERVATION_RETENTION_DAYS` 解析與預設值
- [ ] README／`.env.example` 文件更新
- [ ] 測試（due 判斷、清理）與 `pnpm lint:check`／`pnpm typecheck` 全綠
