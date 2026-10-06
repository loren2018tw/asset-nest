# 規格：網段外觀測（探索時被動 ARP 監聽）

- 狀態：ready-for-agent（2026-10-06）
- 詞彙依 `GLOSSARY.md`「觀測詞彙」；決策見 `docs/adr/0014`（觀測與宣告分離）、`docs/adr/0015`（本地優先探測）、`docs/adr/0016`（觀測記錄模型）、`docs/adr/0017`（被動監聽）。
- 延伸 `.scratch/ip-observation/spec.md`；沿用其 migration 0006 的現況／事件模型與掃描服務。

## Problem Statement

同 L2 上可能有不屬於任何受管網段（或不在探測網段 CIDR 內）的位址在使用——錯設定、私接設備、其他網段延伸。主動探測只送「已指派／租約／CIDR 內 host」位址，這類設備完全不會被發現；被動讀 ARP 是唯一涵蓋手段，但交換器只會轉發廣播，必須在探索時監聽。

## Solution

探索掃描（手動與排程）期間，在被探測網段的介面上被動監聽 ARP，**與主動探測同時進行**（不重複累加時間）；sender 位址落在該網段 CIDR 外者記入觀測影子層（`ip_presence.out_of_subnet=1`、來源 `arp_passive`），並新增「網段外觀測」清單頁。唯讀，永不修改宣告（ADR-0014）。

## User Stories

1. 作為管理者，我想在探索掃描時被動監聽 ARP，以便發現同 L2 但網段外使用中的位址。
2. 作為管理者，我想設定監聽窗長（`OBSERVATION_PASSIVE_WINDOW_SECS`，預設 60 秒，0＝停用），以便在命中率與負擔間取捨。
3. 作為管理者，我想在「網段外觀測」頁看到位址、MAC、首次／最後看到、來源與探測網段，以便調查。
4. 作為管理者，我想未知 MAC 標「未登錄」、已知 MAC 可連到資產（重用 MAC 歷史對話框）。
5. 作為管理者，我想在 unprivileged 模式清楚知道無法被動監聽，而不是默默沒資料。
6. 作為管理者，我想探索報告顯示本輪被動看到的位址數（`passive_seen`）。

## Implementation Decisions

### 資料庫（migration 0007）

- `ip_presence` 新增 `out_of_subnet INTEGER NOT NULL DEFAULT 0`。
  - **只有被動監聽路徑寫 1**；其他路徑（快速／探索主動、Kea 租約）不觸碰此欄，避免同一列在兩種來源間翻轉。
- 部分索引 `idx_ip_presence_out_of_subnet ON ip_presence(subnet_id) WHERE out_of_subnet = 1`（清單查詢用）。
- 事件沿用 `observation_event`：`source='arp_passive'`，`first_seen`／`mac_changed` 與 latest-wins 規則不變（ADR-0016）。因此 MAC 歷史端點自動涵蓋網段外 sighting。

### 探測邊界

- `Prober::passive_observe(&self, subnet: &Subnet, window: Duration) -> Vec<(Ipv4Addr, Mac)>`：
  - **raw（含 auto 的 raw 路徑）**：以 `AF_PACKET`／`ETH_P_ARP` 開監聽 socket（不送任何封包），在窗內解析 opcode 1（request）與 2（reply）的 sender IP／MAC；排除 `0.0.0.0`、multicast／broadcast、本機介面自身位址與 MAC。
  - **unprivileged**：回空（無法被動監聽）；`auto` 降級後同理。
  - 回傳窗內全部合法 sender（不去重、不濾 CIDR）；CIDR 過濾由服務層做（可測）。
- 純函式 `parse_arp_sender(frame) -> Option<(Ipv4Addr, Mac)>` 與過濾邏輯單元測試。

### 掃描服務

- `run_discovery` 增參數 `passive_window: Duration`（`0`＝停用）：
  - 先 `spawn_blocking` 啟動 `passive_observe`，再照現行批次主動探測，最後 join 被動結果。
  - 服務層只保留 CIDR **外** 的 sender；以 `record_seen` 同交易寫入（`out_of_subnet=1`、來源 `arp_passive`、時間 `now`）。
  - 報告新增 `passive_seen`（本輪寫入的相異網段外位址數）。
- 快速掃描不變（不做被動監聽）。

### 環境設定

- `OBSERVATION_PASSIVE_WINDOW_SECS`（預設 60；0＝停用；非負整數）。
- `AppState.passive_window_secs: u64`，比照 `discovery_rate_pps` 以 builder 注入；測試預設值即可。

### HTTP API

- `GET /api/v1/observations/unmanaged`：列出 `out_of_subnet=1` 的現況，last_seen 新→舊：
  `{ items: [{ subnet_id, subnet_cidr, subnet_name, address, mac, first_seen_at, last_seen_at, source, known, asset }] }`
  - `first_seen_at`＝該列最早事件時間（事件清理後退化為 last_seen）；`known`／`asset` 比照 MAC 歷史。
- 無需新寫入端點；資料只由探索掃描產生。

### 前端

- 新頁「網段外觀測」：路由 `/observations/unmanaged`、側邊欄 IP 管理區段新增項目。
- 表格欄位：IP、MAC（未登錄／已知＋資產連結）、首次看到、最後看到、來源、觀測網段；點 MAC 開既有 `ObservationHistoryDialog`（MAC 模式）。
- 空狀態說明：僅 raw 模式、探索掃描時被動監聽；窗長可調。

### 文件

- `.env.example`、`README.md` 補 `OBSERVATION_PASSIVE_WINDOW_SECS` 與功能說明（含命中率取決於設備活動、unprivileged 無資料）。

## Testing Decisions

- 只測外部行為；stub `Prober::passive_observe` 腳本化回傳，任何真實網路行為不進自動測試。
- 規劃測試：
  - 單元：`parse_arp_sender`（opcode 1／2、格式錯誤）、CIDR 外過濾純函式、窗長 0 停用。
  - 整合（stub）：探索寫入 `out_of_subnet=1` 現況與 `first_seen`（來源 `arp_passive`）；CIDR 內 sender 被丟棄；報告 `passive_seen`；端點列出（已知 MAC＋資產連結／未登錄）；重複探索同 MAC 不重複寫事件。
  - 真機（`#[ignore]`）：raw 被動監聽短窗（唯讀）。
- 前端維持 `pnpm lint:check`／`pnpm typecheck`（不新增測試框架）。

## Out of Scope

- 主動探測網段外位址、常駐背景嗅探、快速掃描時監聽（可後續加）。
- 自動告警、自動處置（ADR-0014）。
- 跨 VLAN／遠端 agent。
- 非 ARP 的發現管道（ICMP、mDNS 等）。
