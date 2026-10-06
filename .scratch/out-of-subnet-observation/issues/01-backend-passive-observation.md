# 01: 被動 ARP 監聽與網段外紀錄（後端）

**What to build:** 探索掃描時在被探測網段的介面上被動監聽 ARP（與主動探測並行），把 sender 位址在 CIDR 外者寫入觀測影子層（`ip_presence.out_of_subnet=1`、來源 `arp_passive`），並提供 `GET /api/v1/observations/unmanaged` 清單端點；窗長以 `OBSERVATION_PASSIVE_WINDOW_SECS` 設定（預設 60、0＝停用）。

**Blocked by:** None (can start immediately)

**Status:** done

- [x] migration 0007：`ip_presence.out_of_subnet` 欄位＋部分索引；不動 0006 以前
- [x] `Prober::passive_observe(subnet, window)`：raw（含 auto 的 raw）以 AF_PACKET 監聽 opcode 1／2 的 sender、排除無效位址與本機；unprivileged 回空；純函式解析＋單元測試
- [x] `run_discovery` 增 `passive_window`：並行啟動被動監聽、服務層過濾 CIDR 外、寫入現況（旗標）與事件（`arp_passive`）；報告加 `passive_seen`；窗長 0＝停用
- [x] `OBSERVATION_PASSIVE_WINDOW_SECS` 設定＋`AppState.passive_window_secs`（builder；手動探索帶入）
- [x] `GET /api/v1/observations/unmanaged`：last_seen 新→舊，含探測網段、首見、known／asset
- [x] `.env.example`／README 更新
- [x] 整合測試（stub）：寫入與事件、CIDR 內丟棄、passive_seen、端點（已知／未登錄）、unprivileged 空；真機 `#[ignore]` 短窗唯讀
- [x] `cargo test`／`cargo fmt --check` 全綠

## Comments

實作摘要（commit `1540dbc`）：

- migration 0007：`ip_presence` 加 `out_of_subnet INTEGER NOT NULL DEFAULT 0`＋部分索引 `idx_ip_presence_out_of_subnet(subnet_id) WHERE out_of_subnet = 1`；0001–0006 未動。
- 探測邊界（`probe.rs`）：
  - `Prober::passive_observe(&self, subnet, window)`（同步、無預設實作；5 個既有測試 stub 皆補空實作）。raw／auto 開 `AF_PACKET`／`ETH_P_ARP` 只收不送，窗內輪詢解析；unprivileged 與非 Linux 回空；失敗（如缺 CAP_NET_RAW）記一次警告。
  - 抽出共用 `parse_arp_frame`；`parse_arp_reply` 行為不變（仍只收 opcode 2），新增 `parse_arp_sender`（opcode 1／2）。`valid_passive_sender` 排除 `0.0.0.0`、multicast、有限／網段廣播與本機位址／MAC。
- 掃描服務（`observation.rs`）：
  - `run_discovery` 增 `passive_window: Duration`（0＝停用、不呼叫 stub）：先 `spawn_blocking` 啟動被動、主動批次照跑、最後 join；`out_of_subnet_senders`（純函式）濾 CIDR 外，`passive_seen`＝實際寫入的相異位址數（MAC 無效者不計）。
  - `apply_results` 增被動 sender 參數（快速掃描固定空切片）；`record_seen` 重構為 `record_observation(..., out_of_subnet)` 共用實作，`record_passive_seen` 是唯一寫 `out_of_subnet = 1` 的路徑（其他路徑的 INSERT／UPDATE 不含該欄，不翻轉）。
  - `SweepReport.passive_seen: u64` 一律序列化（快速掃描固定 0，已於欄位註解說明）。
  - `unmanaged_observations`：`out_of_subnet = 1` join subnets，`last_seen_at DESC`（同時間依 subnet_id、address）；`first_seen_at = COALESCE(MIN(事件時間), last_seen_at)`；`known`／`asset` 重用模組內 `assets_for_macs`。
- 設定／狀態：`OBSERVATION_PASSIVE_WINDOW_SECS`（非負整數、預設 60、0＝停用；`parse_passive_window_secs`）＋`Config.observation_passive_window_secs`；`AppState.passive_window_secs`（預設 60）＋`with_passive_window_secs`；`main.rs` 帶入；手動探索（`api/subnets.rs`）與排程器皆傳 `Duration::from_secs(state.passive_window_secs)`，排程 log 加 `passive_seen`。
- HTTP：`GET /api/v1/observations/unmanaged` 回 `{ items: [...] }`；item＝`{subnet_id, subnet_cidr, subnet_name, address, mac, first_seen_at, last_seen_at, source, known, asset?}`（`asset` 未知 MAC 時省略）。
- 文件：`.env.example` 與 README「IP 觀測」新增變數說明與功能段落（僅 raw 有資料、窗長影響命中率、歸屬探測網段）。
- 測試：新增 `backend/tests/observation_passive.rs`（6 個整合測試＋1 個 `#[ignore]` 真機短窗唯讀）；`probe.rs` 加 5 個單元測試（parse opcode 1／2、malformed、sender 過濾、unprivileged／零窗回空）；`observation.rs` 加 2 個單元測試（CIDR 外過濾、旗標不被一般路徑清除）；`config.rs` 加 2 個（解析、預設）。
- 驗證：`cargo test` 313 passed／0 failed／7 ignored；`cargo fmt --check`、`pnpm lint:check`、`pnpm --filter frontend typecheck` 全綠（前端未改）。

給票 02（前端）的介面備註：

- 端點：`GET /api/v1/observations/unmanaged` → `{ "items": [...] }`，已依 `last_seen_at` 新到舊。
- item 欄位：`subnet_id: number`、`subnet_cidr: string`、`subnet_name: string | null`、`address: string`、`mac: string | null`、`first_seen_at: string`（UTC `YYYY-MM-DDTHH:MM:SSZ`）、`last_seen_at: string`、`source: string | null`、`known: boolean`、`asset?: {id, description, location, property_no: string | null}`。
- 點 MAC 開既有 `ObservationHistoryDialog`（MAC 模式）沿用 `GET /api/v1/observations/mac/{mac}`；`mac` 為 null 時無歷史可開。
- 掃描報告新增 `passive_seen: number`（快速與探索皆有；探索才有非零值）。
