# 03: 被動 sender 入庫（網段外觀測）

**What to build:** observations 端點追加 passive 報告（sender 清單）：只記錄落在該代理網段 CIDR 外的 sender（CIDR 內丟棄），寫入觀測影子層的「網段外觀測」（來源 `arp_passive`）；「網段外觀測」頁與 MAC 歷史（含未登錄標示、資產連結）自動反映；重複回報不重複寫事件。

**Blocked by:** 02

**Status:** done

- [x] passive 報告：CIDR 外 sender 寫入（`out_of_subnet=1`、來源 `arp_passive`）；CIDR 內丟棄
- [x] 事件推導沿用 latest-wins（首見／MAC 變更）
- [x] 「網段外觀測」清單顯示新資料；MAC 未登錄／已知資產連結沿用既有 UI
- [x] 整合測試：只收 CIDR 外、重複不重複事件、與 sweep 報告並存語意；全綠

## Comments

實作完成（未 commit；依票 03 範圍，僅入庫端 passive 擴充與測試；讀取端沿用既有 `out_of_subnet` 列）。

- `backend/src/api/agents.rs`：`reports` 輸入改為共用容器 `ReportInput`（`checked`／`seen`／`senders` 皆可選），`kind` 決定採用欄位：`sweep` 驗證 `checked`／`seen`、`passive` 驗證 `senders`，未知 kind 仍回 400 並標示 `reports[i].kind`。驗證產物改為 `ReportEntry`（`Sweep`／`Passive`）；sender 沿用票 02 慣例回 400 標示 `reports[i].senders[j].address|mac`；`observed_at` 解析抽出 `require_observed_at`。
- `backend/src/agents.rs`：新增 `PassiveReport`、`ReportEntry` enum（`observed_at()` 為排序鍵）；`ObservationReport.reports` 改為混批。`record_observations` 跨 kind 依 `observed_at` 舊到新排序、在同一交易套用（未來值夾制不變）：sweep 照舊；passive 的 sender 落在代理網段 CIDR 外才以 `record_passive_seen` 寫入（來源 `arp_passive`、`out_of_subnet=1`），CIDR 內丟棄（ADR-0017），成功後照舊更新 `last_observation_at`。
- `backend/src/observation.rs`：`record_passive_seen` 改 `pub(crate)` 供代理入庫呼叫（事件推導、latest-wins 沿用同一 `record_observation` 實作）；doc 更新 CIDR 過濾呼叫端。
- `backend/tests/agent_ingest.rs`：原「passive 尚未支援」測試改為未知 kind 仍 400；新增 3 個整合測試：CIDR 外寫入（旗標、來源、MAC 正規化）+ CIDR 內（含網段位址）丟棄 + `GET /api/v1/observations/out-of-subnet` E2E（未登錄／已知資產連結）、重複 passive 不重複事件與較舊補送 latest-wins、sweep＋passive 混批同交易排序落地與清單只含被動列；驗證案例補 2 例（passive sender 非 IPv4／非法 MAC）。
- 驗收：`cargo test` 全綠（338 passed、0 failed、7 ignored，agent_ingest 由 8 增至 11）；`cargo fmt --check` 綠；`cargo clippy` 本次變更檔案無警告；`pnpm lint:check`、`pnpm --filter frontend typecheck` 全綠。
- 備註：「網段外觀測」清單與 MAC 歷史為既有讀取端，passive 列自動反映（未動 UI）；`passive_seen` 統計欄位屬舊本機探索端點，代理回報不變更該端點語意。
