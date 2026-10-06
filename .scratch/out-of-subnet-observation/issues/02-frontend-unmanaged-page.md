# 02: 網段外觀測清單頁（前端）

**What to build:** 新增「網段外觀測」頁（路由 `/observations/unmanaged`、側邊欄項目），列出被動監聽到的網段外位址；未知 MAC 標「未登錄」、已知 MAC 顯示資產並可點開既有觀測歷史對話框。

**Blocked by:** 01 被動 ARP 監聽與網段外紀錄（後端）

**Status:** ready-for-agent

- [ ] `frontend/src/api/observations.ts` 新增清單型別與 `listUnmanagedObservations()`
- [ ] 新頁 `ObservationsUnmanagedPage.vue`：欄位 IP／MAC／首次看到／最後看到／來源／觀測網段；last_seen 相對時間；點 MAC 開 `ObservationHistoryDialog`（MAC 模式）
- [ ] 側邊欄（IP 管理區段）與路由掛載
- [ ] 空狀態說明：僅 raw 模式、探索時監聽、窗長可調（unprivileged 無資料）
- [ ] `pnpm lint:check`／`pnpm --filter frontend typecheck` 全綠
