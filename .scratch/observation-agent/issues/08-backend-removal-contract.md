# 08: 後端移除與語意收斂（contract）

**What to build:** 代理路徑完整後，收掉後端探測能力與雙軌語意：移除本機探測模組、掃描服務與排程器、掃描 API 與網段觀測設定（觀測開關、探索開關／間隔、上次探索）、本機判定與探測相關環境變數、systemd 特權；migration 移除網段觀測欄位；「未觀測」改為僅以在線代理判定；每日清理任務（事件＋被拒回報）進主程式；前端移除網段觀測區塊與「立即掃描／立即探索」；README／`.env.example` 觀測章節改寫為代理模型。既有測試改寫或移除。

**Blocked by:** 06

**Status:** done

- [x] 後端不再發送任何探測；探測模組、掃描服務／排程器、掃描 API 與其測試移除
- [x] 網段 API／回應不再有觀測設定與本機欄位；migration 移除四個網段觀測欄位
- [x] 探測相關環境變數移除；systemd 無 CAP；保留事件保留設定並新增被拒回報保留設定（預設 30 天）
- [x] 「未觀測」＝所屬網段沒有在線代理（v6 恆未觀測）；雙軌判定收斂為單軌並有測試
- [x] 每日清理（事件＋被拒回報）於主程式執行；測試覆蓋
- [x] 前端無觀測設定／立即掃描 UI；`pnpm lint:check`／`pnpm typecheck` 全綠；後端測試全綠
- [x] README／`.env.example` 與現況一致

## Comments

實作完成（未 commit）。後端不再有任何探測路徑：刪除 `backend/src/probe.rs`（1102 行）與 `backend/src/observation/scheduler.rs`；`libc` 自 `backend/Cargo.toml` 移除（確認僅 `probe.rs` 使用）；systemd 單元移除 `AmbientCapabilities`／`CapabilityBoundingSet`（其餘硬化保留）。租約觀測改由主程式薄週期任務執行；「已觀測」收斂為「v4 ∧ 該網段有在線代理」單軌。

### 變更檔案

- **後端**：`main.rs`（新 `spawn_maintenance`：租約每 15 分鐘、清理每日＋啟動；`tokio::select!` 兩週期不重疊）、`lib.rs`（`AppState` 移除 `prober`／`discovery_rate_pps`／`passive_window_secs` 與 builder）、`observation.rs`（移除 `run_quick`／`run_discovery`／`probe_rate_limited`／`touch_last_discovery`／`validate_quick`／`validate_discovery`／`assigned_targets`／`apply_results`／`SweepReport` 與限速純函式；新增 `record_lease_observations`；`effective_coverage` 移除 prober 參數；`normalize_mac` 移入為 `pub(crate)`）、`agents.rs`（新增 `cleanup_auth_failures`）、`config.rs`（移除 4 個探測變數；新增 `AGENT_AUTH_FAILURE_RETENTION_DAYS` 預設 30＋單元測試）、`subnets.rs`（`Subnet`／`SubnetSummary`／`SubnetPatch`／`ValidSubnet`／SQL 全面移除觀測欄位；`list` 移除 prober 與 `local`）、`api/subnets.rs`（移除 `POST /subnets/{id}/sweeps` 與 `SweepInput`；`SubnetDetail` 包裝結構移除，回應即 `Subnet`）、`api/ips.rs`／`api/observations.rs`／`api/agents.rs`（呼叫端與 MAC 正規化來源同步）、`conflicts.rs`／`ips.rs`（測試用 `Subnet` 建構子同步）、`Cargo.toml`／`Cargo.lock`（移除 `libc`）。
- **migration**：新增 `backend/migrations/0009_remove_subnet_observation_columns.sql`（`ALTER TABLE subnets DROP COLUMN` × 4：`observed`、`discovery_enabled`、`discovery_interval_minutes`、`last_discovery_at`）。
- **測試**：刪除 `observation_sweep.rs`、`observation_discovery.rs`、`observation_passive.rs`、`observation_settings.rs`；改寫 `observation_lease.rs`（6 測試：有效租約以 cltt 記最後可見、缺 cltt／MAC 與非 default／非對應／非 IPv4 略過、多網段各依 `kea_subnet_id`、與 ARP 的 latest-wins、舊租約不重複寫事件、未設 Kea 不發命令、Kea 失敗不影響既有資料）；`observation_retention.rs` 擴充被拒回報清理（邊界＋no-op）；`observation_history.rs` 改以植入在線代理測涵蓋（含代理離線回到未觀測）；`agent_ingest.rs` 移除 `StubProber`（未觀測判定改直接驗無代理／代理過期）。
- **前端**：`SubnetFormDialog.vue`（移除觀測區塊與所有相關 state／imports）、`IpListPage.vue`（移除掃描工具列與停用原因）、`api/ips.ts`（移除 `quickSweep`／`SweepReport`）、`api/subnets.ts`（移除 `discoverySweep`／`SweepReport` 與觀測型別欄位）、`SubnetsPage.vue`（移除觀測欄）、`api/observations.ts`／`ObservationHistoryDialog.vue`／`ObservationsOutOfSubnetPage.vue`（文案同步代理語意）。
- **文件／安裝**：`README.md`（「IP 觀測」改寫為代理模型＋環境變數表；「觀測代理」章節保留）、`.env.example`（單一觀測區塊列 `AGENT_AUTH_CODE`／`AGENT_STALE_SECS`／`AGENT_AUTH_FAILURE_RETENTION_DAYS`／`OBSERVATION_RETENTION_DAYS`）、`deploy/install.sh`（移除 CAP）。

### 實作決策（票未明說者）

- **租約任務順序**：先讀完所有網段租約（HTTP）再開單一交易寫入，避免在網路 I/O 期間持有 SQLite 寫鎖（比照舊快速掃描「先讀租約、後寫入」）。不做 `upsert_checked`：已無探測，不應標記 `last_checked_at`；依票文只以 `cltt` 記 `record_seen(..., "kea_lease", cltt)`。缺 `cltt` 不記（無可信時間）、缺 MAC 仍記時間與來源。
- **被拒回報清理界線**：`last_attempt_at < now - retention_days`（嚴格早於；恰在 cutoff 保留），與事件清理同語意；首次嘗試很久但最近仍在嘗試的來源不會被清掉。
- **`normalize_mac` 去處**：原在 `probe.rs`（接受 6／8 組十六進位，與 `interfaces::normalize_mac` 的 12 位規則不同），移除探測模組後移入 `observation.rs` 為 `pub(crate)`；`Mac` type alias 不再需要。
- **`SubnetDetail`**：移除 `local` 後只剩扁平的 `subnet`，直接刪除包裝結構，JSON 形狀不變。
- **額外文案同步**（探測時代殘留）：`ObservationsOutOfSubnetPage.vue` 空狀態（探索掃描／raw 模式／`OBSERVATION_PASSIVE_WINDOW_SECS` → 代理被動監聽）、`ObservationHistoryDialog.vue`「未觀測」提示；agent crate 真機 `#[ignore]` 訊息「本機同 L2」改為「與目標網段同 L2」（語意更精確，非行為變更）。

### 驗證結果

- `cargo test --manifest-path backend/Cargo.toml`：**301 passed、0 failed、5 ignored**（ignored 為 Kea 真機測試）。
- `cargo fmt --manifest-path backend/Cargo.toml -- --check`：OK。
- `cargo clippy --manifest-path backend/Cargo.toml --all-targets`：無 error；本次新改動零警告。全庫仍有 16 個既有 warning（`assignments`／`conflicts`／`import`／`ips` 篩選段／`kea/sync`／`subnet_import`／`subnets::ensure_no_conflicts` 的 `collapsible_if` 等，皆在未改動行）；另修掉 `observation_retention.rs` 既有的 `type_complexity`（該檔在本次 diff 中）。
- `pnpm test:agent`：**55 passed、0 failed、3 ignored**。
- `pnpm lint:check`、`pnpm --filter frontend typecheck`：全綠。
- **grep 殘留檢查**（`backend`／`frontend`／`deploy`，排除 `target/`）：`is_local`／`本機同 L2`／`非同 L2`／`立即掃描`／`立即探索`／`probe`／`discovery`／`StubProber`／`with_prober`／`sweeps` 皆無殘留；唯一保留「沒有『立即掃描』」是 README 的正向說明句。`OBSERVATION_PROBE_MODE`／`OBSERVATION_DISCOVERY_*`／`OBSERVATION_PASSIVE_WINDOW_SECS` 於前後端、腳本與文件皆無殘留（僅 superseded ADR-0017 依指示未動）。
- **migration**：0009 於所有測試的 `sqlx::migrate!` 路徑實際套用通過；`subnets` 插入／更新不再引用已移除欄位。

### 待決策／已知邊界

- 現有部署升級後，migration 0009 直接丟棄四個網段觀測欄位；「已觀測」在安裝代理前歸零（ADR-0018 已載明為刻意邊界）。若需保留舊設定值供回退，需另開票處理（本票依規格不移轉）。
- 清理任務無獨立測試直接覆蓋 `spawn_maintenance` 迴圈本身（binary 內），但兩個清理函式與租約函式皆有整合／單元測試；週期與 select 行為以程式碼審閱確認。
