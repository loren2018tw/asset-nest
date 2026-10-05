# 07: 衝突標記與篩選

**What to build:** IP 清單出現兩個新標記——已指派位址的宣告 MAC 與觀測 MAC 不符（`ObservedMacMismatch`）、未指派且非池內位址有主（`ObservedOnUnassigned`）；並可篩選「非法佔用 IP」「有未登錄 MAC」。池內位址被 DHCP 正常使用時不標記。

**Blocked by:** 02 快速掃描與「最後可見」欄（核心）

**Status:** done

- [x] 兩標記即時計算（不落地），以 IP 列徽章與說明呈現
- [x] 宣告介面無 MAC 時不比對；池內位址不標 `ObservedOnUnassigned`
- [x] 篩選為伺服器端（`unassigned_seen`、`unknown_mac`；MAC 比對不分大小寫）
- [x] 測試與 `pnpm lint:check`／`pnpm typecheck` 全綠

## Comments

實作摘要（commit `92a6e9d`）：

- 後端（觀測碼即時計算、不落地；`detect`／`warnings_for` 不動，網段衝突數與指派警示不受影響）：
  - `conflicts.rs` 新增 `OBSERVED_MAC_MISMATCH`／`OBSERVED_ON_UNASSIGNED` 常數與 `observed_codes(subnet, assignments, presence)`（以位址文字索引；比對不分大小寫）。
  - `ips.rs` 新增 `IpObservedFilter`（`unassigned_seen`／`unknown_mac`）與 `IpFilter.observed`；`list_v4` 以 `merged_conflicts` 合併既有指派語意碼＋觀測碼（既有三碼在前、觀測碼接續；未指派列亦可能帶 `ObservedOnUnassigned`）；`matches_v4_filters` 同時供串流掃描與排序鍵路徑使用，預設快速路徑（精確位址、算術位移）在帶 `observed` 篩選時自動繞過；v6 帶 `observed` 篩選一律回空集合。
  - `ObservationView` 新增 `known_macs`；`interfaces::macs` 提供全系統 Interface MAC（trim＋小寫）供 `unknown_mac` 比對；`api/ips.rs` 的 `ListQuery` 解析 `observed` 白名單（無效值 400、`details.field=observed`），僅 `unknown_mac` 查詢才多讀 MAC 集合。
- 前端：`IpListPage.vue` 新增可清除的「觀測」篩選下拉（v6 隱藏）並送 `observed=`；`conflictInfo` 新增「觀測 MAC 不符」「未指派有主」徽章與 tooltip（皆註明僅提示、不阻擋）；`api/ips.ts` 新增 `IpObservedFilter` 型別、`IpListParams.observed` 與 `IpEntry.conflicts` 註解。
- 測試：`backend/tests/ips.rs` 新增 5 個整合測試（不符／同 MAC 大小寫／無宣告 MAC、未指派有主與池內排除、合併排序、伺服器端篩選與排序分頁及 q／status 組合、v6 空集合與無效值 400、觀測碼不計入網段衝突數且指派警示不含觀測碼）；`conflicts.rs`＋`ips.rs` 新增 3 個單元測試。`cargo test` 293 passed／0 failed／6 ignored；`cargo fmt --check`、`pnpm lint:check`、`pnpm --filter frontend typecheck` 全綠。

後續票 08 備註：

- 兩個觀測碼定義在 `backend/src/conflicts.rs`（`OBSERVED_MAC_MISMATCH`／`OBSERVED_ON_UNASSIGNED`），`observed_codes` 僅供 IP 清單列合併；資產端如需比照勿重用 `detect`（語意不同）。
- 資產 `last_seen` 聚合可重用 `observation::presence_map`（單網段）與 `interfaces::macs`（MAC→已知集合）；「介面 MAC 命中未指派位址」目前沒有現成跨網段查詢，需新查詢：`ip_presence` 以 `LOWER(last_seen_mac) IN (介面 MAC)` 或先取資產所有 MAC 再彙總 `MAX(last_seen_at)`；「已知 MAC 連資產」可重用 `observation::assets_for_macs`（現為私有，需提升可見度或走 `/api/v1/observations/mac/{mac}`）。
- `IpEntry.conflicts` 順序保證為既有指派碼在前、觀測碼在後；`sort=last_seen` NULL-last 語意不變。
