# 05: 探索掃描與未知設備

**What to build:** 管理者可在網段開啟探索掃描並設定間隔（預設每日，可用 `OBSERVATION_DISCOVERY_INTERVAL_SECS` 調整）；探索對全網段 host 位址限速探測（`OBSERVATION_DISCOVERY_RATE_PPS`），發現未指派卻有主的位址與未登錄 MAC；表單顯示上次探索時間；可手動立即探索。

**Blocked by:** 04 背景排程與保留清理

**Status:** done

- [x] 網段 PATCH 支援 `discovery_enabled`（需先開觀測，否則 400）與探索間隔（正整數或空＝全站預設）
- [x] 探索目標＝該網段全部 host 位址（測試以 stub 斷言），依速率限速發送；更新 `last_discovery_at`
- [x] 未指派被看見者建立現況列、未看見者不建列；事件規則同快速掃描
- [x] 排程依網段間隔觸發探索；`POST /subnets/{id}/sweeps` 支援 mode=discovery
- [x] 網段表單探索區塊（開關／間隔／上次探索／立即探索）；`.env.example` 更新
- [x] 測試與 `pnpm lint:check`／`pnpm typecheck` 全綠

## Comments

實作完成（主實作 commit `7475abf`，`05 IP 觀測：探索掃描與未知設備`；未新增 migration，沿用 0006 欄位）。

- **領域／API**：`Subnet` 新增 `discovery_enabled`／`discovery_interval_minutes`／`last_discovery_at`（`COLUMNS`、`SubnetRow`、`into_subnet`、`update` 一併擴充；`last_discovery_at` 只由掃描寫入）。`SubnetPatch` 新增 `discovery_enabled` 與 `discovery_interval_minutes: Option<Option<i64>>`（`double_option`：未提供＝維持、`null`＝清除、值＝設定）；`apply_to` 以合併後狀態驗證：探索需 v4、需 `observed=true`（含「關觀測但探索仍開」被擋）、間隔須正整數（`≤0` 回 400，非整數由 JSON 反序列化擋下）。詳情／PATCH／建立回應自動帶新欄位；列表摘要維持 observed/local（測試斷言摘要無探索欄位）。
- **設定**：`Config` 新增 `observation_discovery_interval_secs: u64`（`OBSERVATION_DISCOVERY_INTERVAL_SECS`，預設 86400）與 `observation_discovery_rate_pps: u32`（`OBSERVATION_DISCOVERY_RATE_PPS`，預設 1000）；兩者皆須正整數，0／非整數回明確錯誤。`AppState` 新增 `discovery_rate_pps`（預設 1000）與 `with_discovery_rate_pps` builder，`main` 由設定帶入；手動探索 handler 讀 `state.discovery_rate_pps`。`.env.example` 補兩變數。
- **`run_discovery`**（`backend/src/observation.rs`）：簽名 `run_discovery(pool, prober, kea, subnet, rate_pps: u32, now)`。前提 v4＋observed＋discovery_enabled＋is_local（服務端再驗一次）。目標＝`HostRange::of(&Ipv4Net)` 全 host；`probe_rate_limited` 以 `rate_pps` 分批（每批一次 `spawn_blocking` probe），批間睡 1 秒、最後一批不睡（單一批次零睡眠，測試快）。寫入重用 `apply_results`（參數改為 `checked_targets`）：快速傳全部目標、探索只傳已指派目標，所以未指派未回應不建列；Kea 租約先寫、ARP 後寫、latest-wins 與事件轉移不變。成功後 `touch_last_discovery` 更新 `last_discovery_at`。`SweepReport` 新增 `last_discovery_at: Option<String>`（`skip_serializing_if`：quick 回應不變，discovery 另回）。
- **HTTP**：`POST /subnets/{id}/sweeps` 的 `mode` 支援 `quick`／`discovery`（未開探索→400 `details.field=discovery_enabled`；v4／observed／local 錯誤沿用服務驗證）。
- **排程**：`spawn_scheduler(state, retention_days, discovery_interval_secs)`；每輪在同一個 `for subnet` 內先快速後探索（依序執行＝同一時間僅一掃描）。探索條件 observed∧v4∧local∧`discovery_enabled`；`discovery_interval(subnet, default_secs)`：`discovery_interval_minutes`（分鐘、正整數）優先，否則全站秒數（非正覆寫防禦性回退，避免熱迴圈）。`last_discovery` map 成功／失敗都記時間（`last_discovery_at` 僅成功寫入）；失敗只 `warn!`。`due` 共用既有純函式。
- **前端**：`api/subnets.ts` 的 `Subnet`／`SubnetInput` 加欄位，新增 `SweepReport` 與 `discoverySweep`。`SubnetFormDialog.vue`：IPv4＋既有＋已開觀測＋同 L2 時顯示探索區塊（開關、間隔分鐘輸入〔留空＝全站預設、正整數規則〕、上次探索相對時間、立即探索按鈕與結果 notify；關閉觀測時 `toInput` 一併送 `discovery_enabled=false` 符合後端合併驗證）。`IpListPage.vue`：`discovery_enabled` 時在「立即掃描」旁多一顆「探索掃描」按鈕（同步執行、notify 並更新 `last_discovery_at`）。`SubnetsPage.vue` 未動：摘要依 ticket 範圍維持 observed／local，無探索欄位可顯示。
- **測試**：新增 `backend/tests/observation_discovery.rs` 4 個整合測試（全 host 範圍探測＋`last_discovery_at` 往返；指派未見→僅 `last_checked`、未指派未見→無現況列、未指派被見→現況＋first_seen、第二輪 mac_changed；未開探索／非同 L2／v6 被拒且不探測；PATCH 驗證：未開觀測被拒、120 往返、`null` 清除、0／-3／1.5／"60" 被拒、關探索後才能關觀測、v6 被拒、摘要不含探索欄位）。`observation_sweep.rs` 既有「discovery 尚未支援」斷言更新為「未開觀測→400」。單元：`config` 解析／預設 2 個、`scheduler` 覆寫與 due 2 個、`subnets` patch 規則 1 個。驗收：`cargo fmt --check` 綠；`cargo test` **281 passed／0 failed／6 ignored**（本功能 126 單元＋155 整合，較票 04 多 9）；`pnpm lint:check`、`pnpm --filter frontend typecheck` 綠。
- **給票 06（歷史對話框／CSV）**：
  - 現有讀取 helper：`observation::presence_map(pool, subnet_id) -> HashMap<String, Presence>`（單查詢批次）、`Presence{last_seen_at,last_seen_mac,last_seen_source,last_checked_at}`、`observation::timestamp(now)`；事件目前無讀取 API，票 06 需新查 `observation_event`（索引 `(subnet_id,address,observed_at)`／`(mac,observed_at)` 已備）。
  - 掃描回應形狀：quick＝`{mode:"quick",targets,seen,duration_ms}`；discovery＝多 `last_discovery_at`（UTC 字串）。錯誤：400 `{error:"validation_error", message, details.field?}`。
  - `Subnet` 已帶探索三欄；`HostRange` 可直接重用於任何枚舉需求。

