# 02: 網段外觀測清單頁（前端）

**What to build:** 新增「網段外觀測」頁（路由 `/observations/out-of-subnet`、側邊欄項目），列出被動監聽到的網段外位址；未知 MAC 標「未登錄」、已知 MAC 顯示資產並可點開既有觀測歷史對話框。

**Blocked by:** 01 被動 ARP 監聽與網段外紀錄（後端）

**Status:** done

- [x] `frontend/src/api/observations.ts` 新增清單型別與 `listOutOfSubnetObservations()`
- [x] 新頁 `ObservationsOutOfSubnetPage.vue`：欄位 IP／MAC／首次看到／最後看到／來源／觀測網段；last_seen 相對時間；點 MAC 開 `ObservationHistoryDialog`（MAC 模式）
- [x] 側邊欄（IP 管理區段）與路由掛載
- [x] 空狀態說明：僅 raw 模式、探索時監聽、窗長可調（unprivileged 無資料）
- [x] `pnpm lint:check`／`pnpm --filter frontend typecheck` 全綠

## Comments

實作摘要（commit `1168cf5`）：

- API（`frontend/src/api/observations.ts`）：新增 `UnmanagedObservation`（比照後端 `{subnet_id, subnet_cidr, subnet_name, address, mac, first_seen_at, last_seen_at, source, known, asset?}`）、`UnmanagedObservationPage` 與 `listUnmanagedObservations()`（`GET /api/v1/observations/unmanaged`）。
- 來源型別（`frontend/src/api/ips.ts`）：`IpSeenSource` 增 `arp_passive`（後端觀測事件與現況已會回傳此值）；`IpListPage.vue` 與 `ObservationHistoryDialog.vue` 的 `sourceLabel` 同步補「ARP 被動」，避免既有歷史對話框顯示「—」。
- 新頁 `frontend/src/pages/ObservationsUnmanagedPage.vue`：
  - 掛載即載入＋「重新整理」按鈕；載入中 `:loading`、錯誤 `$q.notify`（沿用既有頁面模式）。
  - 表格：IP（monospace）、MAC（monospace 可點按鈕 → MAC 模式 `ObservationHistoryDialog`；`known=false` 顯示「未登錄」badge，`known=true` 顯示 `財產編號(描述) ｜ 位置`；`mac=null` 顯示「—」）、首次看到／最後看到（`relativeTime`＋精確時間 tooltip）、來源、觀測網段（名稱 ｜ CIDR，monospace）。
  - 列鍵為 `subnet_id-address`（同一 L2 多個探測網段各有一列，ADR-0017）；後端已排序，不分頁。
  - 空狀態列出：僅探索掃描被動監聽（快速掃描不監聽）、需 raw 模式（unprivileged 降級不會有資料）、窗長 `OBSERVATION_PASSIVE_WINDOW_SECS`（預設 60 秒、0＝停用）、閒置設備可能不出現。
- 路由（`frontend/src/router/routes.ts`）：新增子路由 `observations/unmanaged`。
- 側邊欄（`frontend/src/layouts/MainLayout.vue`）：IP 管理項目之後新增「網段外觀測」（`travel_explore`），以獨立 `unmanagedActive` computed 高亮；`ipSection` 與 `to="/ips"` 行為不變。
- 驗證：`pnpm lint:check`、`pnpm --filter frontend typecheck`、`pnpm --filter frontend build` 全綠。

給後續驗證的備註：

- 來源欄除 `arp_passive`（顯示「ARP 被動」）外亦處理 `arp`／`kea_lease`／null：`out_of_subnet=1` 的列若後續被主動掃描（如快速掃描已指派網段外位址）更新，`last_seen_source` 可能不再是 `arp_passive`。
- 對話框沿用 `GET /api/v1/observations/mac/{mac}`，網段外 sightings 會自動納入 MAC 歷史。
- 未動後端、`.env` 或無關檔案。
