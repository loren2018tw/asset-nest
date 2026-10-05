# 02: 快速掃描與「最後可見」欄（核心）

**What to build:** 管理者在已開啟觀測的網段按「立即掃描」，系統對該網段的已指派位址做 ARP 探測，記錄每個位址的最後可見時間、最後 MAC 與最後檢查時間；IP 清單多一欄「最後可見」（N 天前／從未上線／未觀測，可排序，提示顯示來源與最後檢查時間）。位址首次被看到與換 MAC 會留下事件（供後續歷史查詢）。本票含 raw ARP 實作與零權限降級、探測模式環境設定，以及 systemd `CAP_NET_RAW` 最小權限。不含租約來源與排程。

**Blocked by:** 01 觀測開關與本機判定

**Status:** done

- [x] 新資料表 `ip_presence` 與 `observation_event` 建立；`subnets` 既有觀測欄位沿用
- [x] 快速掃描僅探測該網段已指派位址（測試以 stub 斷言目標集合）；未回應者更新最後檢查時間、不動最後可見
- [x] 回應者寫入最後可見（來源 `arp`）；首次看到寫 `first_seen`；MAC 變更寫 `mac_changed`；同 MAC 重掃不重複寫事件
- [x] `POST /subnets/{id}/sweeps`（mode=quick）同步回摘要；未開觀測／非同 L2／v6 回 400
- [x] IP 清單回傳最後可見欄位與有效涵蓋狀態；`sort=last_seen`（NULL 固定最後）
- [x] IP 清單前端欄位（三態＋來源提示）與「立即掃描」按鈕
- [x] raw ARP 與零權限降級可由 `OBSERVATION_PROBE_MODE` 選擇（`auto` 遇權限問題降級）；systemd 單元含 `CAP_NET_RAW` 並更新 `.env.example`
- [x] 測試：stub 探測邊界整合測試、ARP 解析單元測試、真機 `#[ignore]` 測試；`pnpm lint:check`／`pnpm typecheck` 全綠

## Comments

實作完成（主實作 commit `ec4cd3c`，`02 IP 觀測：快速掃描、ARP 探測與最後可見欄（migration 沿用 0006）`；migration 沿用 0006、未新增）。

- `backend/src/probe.rs`：`Prober::probe(subnet, targets) -> Vec<(Ipv4Addr, Mac)>`（`Mac`＝正規化小寫冒號字串）；`ProbeMode{auto,raw,unprivileged}`＋`SystemProber::with_mode`。raw 以 `AF_PACKET` 自建 ARP 請求（broadcast、EtherType 0x0806、opcode 1；介面由 getifaddrs 比對網段、`if_nametoindex`、MAC 讀 sysfs），送畢後收集回覆約 2 秒；僅取 opcode 2 且 sender IP 在目標集合者。unprivileged 對目標丟 UDP 觸發 kernel ARP 解析、等待 250ms 後讀 `/proc/net/arp`（重用 `peer::mac_from_arp_table`）。auto 遇 EPERM／EACCES 或環境不可用記一次 `warn` 後降級。框架組裝／解析與 MAC 正規化為純函式。
- `backend/src/observation.rs`：`run_quick(pool, prober, subnet, now)`；目標＝已指派位址（票 03 加入租約），所有目標 upsert `last_checked_at=now`，回應者 latest-wins 寫 `last_seen_*`（來源 `arp`）並依 MAC 轉移寫 `first_seen`／`mac_changed`（同 MAC 不寫）；探測走 `spawn_blocking`；摘要 `{mode,targets,seen,duration_ms}`。另提供 `presence_map` 批次讀取與 `ObservationView`（有效涵蓋＋現況，供 IP 清單）。
- `backend/src/api/`：`POST /subnets/{id}/sweeps`（僅 `mode=quick`；未知模式／`discovery`／缺 mode 回 400）；v4／已開觀測／本機同 L2 前提由服務驗證回 400 中文訊息。IP 清單新增 `last_seen_at`／`last_seen_mac`／`last_seen_source`／`last_checked_at` 與有效 `observed`；`sort=last_seen` NULL 固定最後、不分升降冪。
- 前端：`utils/relativeTime.ts`（剛看到／N 分鐘前／N 小時前／N 天前）；IP 清單「最後可見」三態欄（tooltip 顯示來源與最後檢查時間）、可排序；工具列「立即掃描」按鈕（v6 隱藏；未開觀測／非同 L2 停用並以 tooltip 說明）。
- 部署：systemd 單元加 `AmbientCapabilities=CAP_NET_RAW`＋`CapabilityBoundingSet=CAP_NET_RAW`（保留 `NoNewPrivileges`）；`.env.example` 文件化 `OBSERVATION_PROBE_MODE`。
- 測試：`tests/observation_sweep.rs`（4 主動＋1 真機 `#[ignore]`：stub 斷言目標集合、事件轉移、未回應僅更新檢查時間、報告計數、400 前提、清單欄位與 NULL 最後排序）；單元 +7（ARP 框架往返／拒絕、MAC 正規化、事件轉移、時間格式、模式解析）；`observation_settings.rs` stub 補 `probe`。
- 驗收：`cargo fmt --check` 綠；`cargo test` 259 passed／0 failed／6 ignored（含既有 5 Kea 真機）；`pnpm lint:check`、`pnpm --filter frontend typecheck` 綠。
- 真機：`tests/observation_sweep.rs::system_prober_probes_live_lan_read_only`（`#[ignore]`，僅送 ARP 請求、不改設定）以 `cargo test --test observation_sweep -- --ignored --nocapture` 執行。
- 票 03 介接：`run_quick(pool: &SqlitePool, prober: Arc<dyn Prober + Send + Sync>, subnet: &Subnet, now: DateTime<Utc>) -> Result<SweepReport, ApiError>`；擴充 `assigned_targets` 納入租約位址後，以 `record_seen(connection, subnet_id, address, mac, source, observed_at)`（`pub(crate)`）帶來源 `kea_lease`、時間 `cltt` 併入，latest-wins 由同一路徑處理。
