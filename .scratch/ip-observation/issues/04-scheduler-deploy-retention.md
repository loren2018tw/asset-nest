# 04: 背景排程與保留清理

**What to build:** 開啟觀測的網段自動每 15 分鐘快速掃描，不需手動；事件依保留期（預設 365 天，可用 `OBSERVATION_RETENTION_DAYS` 調整）每日自動清理且不影響現況；README 補上新設定與功能說明。

**Blocked by:** 02 快速掃描與「最後可見」欄（核心）

**Status:** done

- [x] 排程器每 60 秒 tick；快速掃描每網段 15 分鐘；同時僅執行一個掃描；啟動後無上次紀錄者視為 due
- [x] 保留清理只刪過期事件、不動現況；以固定 `now` 可測
- [x] 環境變數 `OBSERVATION_RETENTION_DAYS` 解析與預設值
- [x] README／`.env.example` 文件更新
- [x] 測試（due 判斷、清理）與 `pnpm lint:check`／`pnpm typecheck` 全綠

## Comments

實作完成（主實作 commit `05c794e`，`04 IP 觀測：背景排程、保留清理與文件`；未新增 migration）。

- 保留清理：`backend/src/observation.rs` 新增 `cleanup_events(pool, retention_days: u32, now: DateTime<Utc>) -> Result<u64, ApiError>`。`cutoff = now - retention_days`（`checked_sub_signed`，極端值不 panic），`DELETE FROM observation_event WHERE observed_at < ?`：**嚴格早於 cutoff 才刪，恰在 cutoff 的事件保留**；走 `idx_observation_event_observed`、全站不分網段；只動事件，`ip_presence` 與宣告資料完全不變；回傳刪除筆數。
- 排程器：新檔 `backend/src/observation/scheduler.rs`（`observation::spawn_scheduler`）。`spawn_scheduler(state: AppState, retention_days: u32) -> JoinHandle<()>`：單一 tokio task、60 秒 tick（`MissedTickBehavior::Delay`，長掃描後不補跑）；每輪先保留清理（啟動即一次、之後每 24 小時），再依序快速掃描（每網段 15 分鐘常數）。due 判斷為純函式 `due(last_run: Option<DateTime<Utc>>, now, interval)`（`None` 視為 due；`last_run + interval <= now`，恰滿即 due），進程內狀態為 `HashMap<subnet_id, last_quick>` 與 `last_cleanup`；只掃 `observed ∧ v4 ∧ prober.is_local` 的網段，`state.kea.as_ref()` 與 `Utc::now()` 注入 `run_quick`。順序執行保證同時僅一個掃描；失敗（清理／讀網段／掃描）只記 `tracing::warn!`、不中斷迴圈，掃描失敗亦記時間、下一週期再試。`main.rs` 於 serve 前啟動、serve 結束後 `abort()`。
- 設定：`Config.observation_retention_days: u32`（`OBSERVATION_RETENTION_DAYS`，預設 365、須為正整數；`parse_retention_days` 對 0／非整數回明確錯誤）；`.env.example` 加註；README 新增「IP 觀測」章節（本機同 L2、每 15 分鐘自動掃描、raw 自動降級、保留清理、兩個環境變數表）。
- 測試：單元 6（清理界線：早 1 秒刪、恰在 cutoff 保留；清理不動現況；`due`：未跑／14 分／15 分／16 分；保留清理 23h／24h 同一規則；設定解析與預設 365）；整合 `backend/tests/observation_retention.rs` 2（跨網段只刪過期＋界線＋現況完整保留；無過期時刪 0 筆）。驗收：`cargo fmt --check` 綠；`cargo test` 272 passed／0 failed／6 ignored；`pnpm lint:check`、`pnpm --filter frontend typecheck` 綠（前端未動）。
- 票 05（探索掃描）介接：
  - due 判斷與排程迴圈都在 `backend/src/observation/scheduler.rs`：`due(last_run, now, interval)` 為共用純函式；`spawn_scheduler` 的 `for subnet in subnets` 內依序執行，天然滿足「同時僅一個掃描」。
  - 探索排程照抄快速掃描模式：加 `last_discovery: HashMap<i64, DateTime<Utc>>`，以 `subnet.discovery_enabled` 過濾（migration 0006 已有欄位；`Subnet` 尚未帶出，票 05 需擴充 `subnets` 讀取），間隔取 `subnet.discovery_interval_minutes`（分）或全站 `OBSERVATION_DISCOVERY_INTERVAL_SECS`（秒）預設；以 `due(...)` 判斷後呼叫探索。
  - `run_discovery` 建議簽名比照 `run_quick`：`run_discovery(&state.db, state.prober.clone(), state.kea.as_ref(), &subnet, now).await`（實際由票 05 定）；成功後更新 `subnets.last_discovery_at` 並 `last_discovery.insert(subnet.id, now)`；失敗只 `warn`（同樣更新 last_discovery 避免每 tick 重打）。
  - 保留清理與快速掃描區塊不需改動；探索掃描放快速掃描前後皆可，順序執行即不重疊。
