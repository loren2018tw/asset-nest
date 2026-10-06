# 規格：觀測代理（agent 執行觀測、後端只留租約與接收）

- 狀態：ready-for-agent（2026-10-06）
- 詞彙依 `GLOSSARY.md`「觀測詞彙」；決策見 `docs/adr/0018`（觀測外移）、`docs/adr/0019`（共用認證碼）。
- 取代 ADR-0015 的本機探測與 ADR-0017 的探索窗機制；沿用 ADR-0014（觀測與宣告分離）、ADR-0016（記錄模型）、ADR-0006（兩層驗證）。

## Problem Statement

目前系統只能觀測與伺服器同 L2 的網段（ADR-0015）：跨 VLAN／遠端網段一律「未觀測」。觀測能力綁在伺服器上——需要特權、伺服器必須位在目標 L2，且每個網段的觀測設定（觀測開關、探索開關與間隔）與伺服器本機位置狀態（`is_local`）糾纏。想觀測其他網段，唯一作法是讓伺服器進駐該 L2。此外系統沒有登入驗證，觀測入庫若完全開放，任何來源都能寫入。

## Solution

觀測執行全面外移給**觀測代理**：每個要觀測的網段安裝一個輕量代理（獨立安裝腳本），代理持續被動監聽 ARP＋週期全段主動掃描，將原始觀測批次推送回伺服器。

伺服器只留：Kea 租約觀測（來源 `kea_lease`）、觀測入庫（認證碼驗證、CIDR 內外分類、現況／事件推導——ADR-0016 模型不變）、讀取與呈現。

使用者在介面上看到：

- 網段不再需要觀測開關：「已觀測」由該網段的代理健康回報決定；代理失聯顯示「未觀測（代理離線）」。
- 系統狀態頁新增 Agent 區塊：回報中的代理清單、被拒回報清單、未對應網段。
- 沒有「立即掃描」——資料新鮮度以「最後回報時間」呈現。

## User Stories

**觀測部署**

1. 作為管理者，我想在目標網段的一台主機上以一行指令安裝觀測代理（伺服器位址＋認證碼），以便觀測該網段。
2. 作為管理者，我想代理自動偵測預設網段、也能以 `--subnet` 明確指定，以便多網卡或特殊環境下不出錯。
3. 作為管理者，我想重跑安裝腳本即完成代理更新，以便維護多台代理。
4. 作為管理者，我想在安裝後於「系統狀態」頁看到代理（自報名稱、版本、來源 IP、涵蓋網段、最後回報），以便確認部署成功。

**觀測語意**

5. 作為管理者，我想代理持續被動監聽，以便發現未登錄、未指派、甚至不在受管網段內的設備（網段外觀測）。
6. 作為管理者，我想代理週期全段主動掃描，以便安靜但活著的設備仍被驗證存活（區分「檢查過、沒回應」與「沒資料」）。
7. 作為管理者，我想「未觀測」＝沒有健康代理涵蓋、「從未上線」＝有掃描卻從未看到，以便正確解讀。
8. 作為管理者，我想代理失聯時資料標示過期、不被清除，以便網路問題排除後資料仍在。
9. 作為管理者，我想 Kea 租約仍是觀測來源之一，以便 DHCP 裝置的活動訊號持續。

**認證與異常可見性**

10. 作為管理者，我想後端安裝時自動產生認證碼、代理輸入同一組才能回報，以便使用者介面登入完成前也有最低防護。
11. 作為管理者，我想認證碼不符的回報顯示在「系統狀態」頁（來源 IP、次數、最後嘗試、自報名稱／版本），以便找出誤設定或遺留的代理，不讓它繼續佔用頻寬。
12. 作為管理者，我想認證碼正確但網段對應不到的代理顯示「未對應」，以便修正設定或先建立網段。

**保有既有能力**

13. 作為管理者，我想繼續看到最後可見、事件歷史、MAC 視角、衝突標記（含 `ObservedMacMismatch`／`ObservedOnUnassigned`）與網段外觀測，以便既有判讀不變。
14. 作為管理者，我想觀測事件保留期（預設一年）與清理行為不變，以便歷史深度可控。

## Implementation Decisions

### 後端移除範圍

- 移除 `probe` 模組（`backend/src/probe.rs`）與 `AppState` 的 `prober`／`discovery_rate_pps`／`passive_window_secs` 與對應 builder；`Cargo.toml` 移除僅供探測的依賴（`libc`）。
- `observation.rs` 移除 `run_quick`／`run_discovery`／`probe_rate_limited`／`touch_last_discovery`／`validate_quick`／`validate_discovery`／`assigned_targets`／`SweepReport`；保留 `lease_signals`／`filter_lease_signals`（租約）、`transition_event`／`upsert_checked`／`record_seen`／`record_passive_seen`／`record_observation`／`cleanup_events`（入庫與清理）。CIDR host 列舉邏輯（`HostRange` 語意：扣 network／broadcast；`/31`、`/32` 全列）移入 agent。
- `observation/scheduler.rs` 移除；`main` 改跑薄清理任務（每日：事件清理＋被拒回報清理），無掃描職責。
- API：移除 `POST /subnets/{id}/sweeps`（`api/subnets.rs`）；`subnets` PATCH 移除 `observed`／`discovery_enabled`／`discovery_interval_minutes` 欄位與驗證；`SubnetSummary`／`SubnetDetail` 移除 `observed`／`local`／`discovery_*`／`last_discovery_at`。
- `config.rs` 移除 `OBSERVATION_PROBE_MODE`／`OBSERVATION_DISCOVERY_INTERVAL_SECS`／`OBSERVATION_DISCOVERY_RATE_PPS`／`OBSERVATION_PASSIVE_WINDOW_SECS`；保留 `OBSERVATION_RETENTION_DAYS`；新增後端變數見下表。
- `deploy/install.sh` 的 systemd 單元移除 `AmbientCapabilities=CAP_NET_RAW`／`CapabilityBoundingSet=CAP_NET_RAW`。

### 資料庫（migration 0008）

- `subnets` 移除 `observed`、`discovery_enabled`、`discovery_interval_minutes`、`last_discovery_at`（SQLite `DROP COLUMN`）。
- 新增 `agent`：
  - `id TEXT PRIMARY KEY`（代理 `instance_id`）、`name TEXT NOT NULL`、`version TEXT NOT NULL`、`source_ip TEXT NOT NULL`（連線來源，忽略 XFF）、`subnet_cidr TEXT NOT NULL`（代理回報值）、`subnet_id INTEGER NULL REFERENCES subnets(id) ON DELETE SET NULL`、`first_report_at TEXT NOT NULL`、`last_report_at TEXT NOT NULL`、`last_observation_at TEXT NULL`；索引 `(subnet_id)`。
- 新增 `agent_auth_failure`：
  - `source_ip TEXT PRIMARY KEY`、`claimed_name TEXT NULL`、`claimed_version TEXT NULL`、`first_attempt_at TEXT NOT NULL`、`last_attempt_at TEXT NOT NULL`、`attempt_count INTEGER NOT NULL DEFAULT 1`。
- `ip_presence`／`observation_event` 不動；`last_seen_source` 續用 `arp`（代理主動）／`arp_passive`（代理被動）／`kea_lease`。

### 觀測代理（新 crate `agent/`）

- 獨立 crate，binary `asset-nest-agent`；探測實作自 `backend/src/probe.rs` 移入（raw `AF_PACKET`、ARP 組框／解析、`getifaddrs` 介面偵測；`is_local` 併為「預設網段偵測」）；不依賴 libpcap。
- 依賴比照後端精簡：`libc`、`tokio`、`reqwest`（rustls、json）、`serde`／`serde_json`、`tracing`。
- 設定（`/etc/asset-nest-agent/agent.env`）：
  - `AGENT_SERVER_URL`、`AGENT_AUTH_CODE`、`AGENT_INSTANCE_ID`（安裝時產生之 UUID）。
  - `AGENT_SUBNET_CIDR`（空＝自動偵測本機介面；多個候選介面判不出來時拒絕啟動並提示指定 `--subnet`）。
  - `AGENT_NAME`（空＝hostname）、`AGENT_SWEEP_INTERVAL_SECS`（預設 900）、`AGENT_SWEEP_RATE_PPS`（預設 1000）。
- 行為：
  - **持續被動**：`AF_PACKET`／`ETH_P_ARP` 只收不送；解析 opcode 1／2 的 sender IP／MAC；排除 `0.0.0.0`、multicast／broadcast、自身介面位址與 MAC；記憶體聚合（位址→最新 MAC），每 30 秒 flush 有更新者（以 flush 時間為 `observed_at`）。
  - **週期掃描**：每 `AGENT_SWEEP_INTERVAL_SECS` 對 `AGENT_SUBNET_CIDR` 全部 host 位址限速（`AGENT_SWEEP_RATE_PPS`）主動探測（沿用既有「送一批、收約 2 秒回覆窗」邏輯）；回報 `checked`（全數送出者）與 `seen`（有回應者）。
  - **推送**：`POST /api/v1/agents/observations`，`X-Auth-Code` 標頭；每請求合計 ≤ 5000 筆（超出拆批）；失敗重試退避 5s→5min；記憶體佇列上限 10000 筆，滿載丟最舊並記警告；401 記明確錯誤後依退避繼續（不熱迴圈）。重啟不保留佇列（可接受：下一輪掃描自然補）。
  - **心跳**：每 60 秒 `POST /api/v1/agents/heartbeat`；回應 `subnet_matched=false` 時記錄一次警告。
- 服務：`asset-nest-agent.service`（`User=asset-nest-agent`、`AmbientCapabilities=CAP_NET_RAW`、`CapabilityBoundingSet=CAP_NET_RAW`、`NoNewPrivileges=true`、`PrivateTmp`、`ProtectSystem=strict` 等比照後端硬化）。

### HTTP API（後端；新 `api/agents.rs`，掛 `/api/v1`）

- `POST /api/v1/agents/heartbeat`：body `{instance_id, name, version, subnet_cidr}` → upsert `agent`（`source_ip` 取連線來源；`subnet_id` 以 CIDR 正規化後精確比對 `subnets.cidr`）→ `{subnet_matched}`。
- `POST /api/v1/agents/observations`：body
  `{instance_id, name, version, subnet_cidr, reports:[{kind:"sweep","observed_at","checked":[ip...],"seen":[{address,mac}...]}, {kind:"passive","observed_at","senders":[{address,mac}...]}]}`：
  - 認證：比對 `X-Auth-Code`；不符→401，記 `agent_auth_failure`（來源 IP、自報名稱／版本 best-effort、次數）且不寫入觀測；後端未設 `AGENT_AUTH_CODE`→503。
  - 代理與網段：upsert `agent`；CIDR 對不到受管網段→記狀態、不入庫、回 `{stored:false, reason:"subnet_unmatched"}`。
  - 寫入（單一交易、latest-wins）：`checked` 全部 `upsert_checked`；`seen` 寫 `record_seen`（來源 `arp`）；`passive.senders` 落在網段 CIDR **外**者寫 `record_passive_seen`（來源 `arp_passive`、`out_of_subnet=1`），CIDR 內丟棄（同 ADR-0017 規則）；更新 `last_observation_at`。
  - 時間：`observed_at` 未來值夾到現在；`reports` 依 `observed_at` 排序後套用；過去值照收（離線補送）。
- `GET /api/v1/agents`：`{stale_secs, items:[{instance_id, name, version, source_ip, subnet_cidr, subnet_id, subnet_name, first_report_at, last_report_at, last_observation_at, online}]}`；`online`＝`last_report_at` 在 `AGENT_STALE_SECS` 內。
- `GET /api/v1/agents/auth-failures`：`{items:[{source_ip, claimed_name, claimed_version, first_attempt_at, last_attempt_at, attempt_count}]}`。

### 讀取端

- 「未觀測」判定改為：所屬網段沒有 `online` 代理（v6 恆未觀測）；「未觀測」與「從未上線」的區分不變。`ips.rs` 的有效涵蓋註解與計算同步更新（原「網段 observed ∧ 本機同 L2」→「網段有 online 代理」）。
- 網段列表／表單移除觀測欄位與提示（`local`、v1 無法觀測、探索開關與間隔、上次探索）。
- 資產 `last_seen` 彙總、歷史、衝突、匯出、網段外觀測頁全部不變。

### 前端

- `KeaStatusPage.vue`（系統狀態）新增：「觀測代理」表（名稱、來源 IP、涵蓋網段、版本、最後回報、狀態 chip：在線／離線／未對應）與「被拒回報」表（來源 IP、自報名稱／版本、次數、最後嘗試）；附空狀態說明。
- 新增 `api/agents.ts`。
- 移除：`SubnetFormDialog.vue` 觀測區塊；`IpListPage.vue`「立即掃描／立即探索」工具列與停用原因；`api/ips.ts` `quickSweep` 與 `SweepReport`；`api/subnets.ts` `discoverySweep`；`SubnetsPage.vue` 觀測提示；`utils` 若有「未觀測」因 `is_local` 而生的文案一併更新。

### 環境設定（後端）

| 變數 | 預設 | 說明 |
|------|------|------|
| `AGENT_AUTH_CODE` | 安裝時隨機產生 | 代理入庫認證碼；未設定＝入庫端點一律 503 |
| `AGENT_STALE_SECS` | `900` | 代理「在線」門檻（秒；正整數） |
| `AGENT_AUTH_FAILURE_RETENTION_DAYS` | `30` | 被拒回報保留天數（正整數） |
| `OBSERVATION_RETENTION_DAYS` | `365` | 事件保留天數（沿用） |

### 安裝與發佈

- `deploy/agent-install.sh`：Ubuntu 24.04／26.04、root、systemd；`--server-url` 與認證碼（`--auth-code`／`--auth-code-file`／環境變數三選一）必填；`--subnet`／`--name`／`--sweep-interval`／`--rate` 選填；`--version <tag>`（預設 latest）下載 prebuilt binary（`asset-nest-agent-{x86_64|aarch64}`，musl 靜態），`--source-dir` 為自建 fallback；安裝至 `/opt/asset-nest-agent`、寫 env（重跑保留 `AGENT_INSTANCE_ID` 與既有認證碼，除非明確覆寫）、建服務並啟動；結束前做一次 `/api/health` 檢查（可達性）。
- `deploy/agent-uninstall.sh`：停用並移除服務與程式；`--purge` 一併移除 `/etc/asset-nest-agent`。
- 發佈 workflow（`.github/workflows/agent-release.yml`）：tag `agent-v*` 觸發，建置 x86_64 與 aarch64 musl 產物上傳 GitHub Release（x86_64 先、arm64 緊接）。
- `deploy/install.sh`：env 檔新增 `AGENT_AUTH_CODE`（`generate_password`）；systemd 移除 CAP；安裝摘要印出代理安裝範例與讀取認證碼的位置（`grep AGENT_AUTH_CODE /etc/asset-nest/asset-nest.env`）。
- `README.md` 觀測章節改寫為代理模型（安裝、認證碼、覆蓋語意、TLS 建議）；`.env.example` 移除探測變數、新增後端變數表。

## Testing Decisions

- 原則沿用：只測外部行為；記憶體 SQLite＋`sqlx::migrate!`＋`tower::ServiceExt::oneshot`。
- 後端新測試：
  - `tests/agent_ingest.rs`：心跳 upsert 與在線判定；CIDR 對應／未對應；sweep 的 checked／seen 寫入；passive 只收 CIDR 外；認證不符拒收且記 `agent_auth_failure`；未設認證碼 503；未來時間夾制；排序後套用；重複回報不重複寫事件（latest-wins）；`first_seen`／`mac_changed` 推導。
  - `tests/agent_status.rs`：`GET /agents`（online／unmatched）、`GET /agents/auth-failures`；「未觀測」判定（無代理＝未觀測、代理過期＝未觀測）。
  - `tests/agent_retention.rs`：事件與被拒回報的保留清理。
- 移除／改寫：`observation_sweep.rs`、`observation_discovery.rs`、`observation_passive.rs`、`observation_settings.rs`（掃描與設定段落退場；被動／網段外語意改由 agent_ingest 覆蓋）；真機 `#[ignore]` 主動／被動測試移往 agent crate。
- Agent crate：單元（ARP 組框／解析、sender 過濾、CIDR host 列舉、批次切分、佇列上限與丟棄、退避）；整合以本機 stub server 驗證心跳、推送、401 行為、未對應回應；真機 `#[ignore]`（raw 探測、被動監聽）比照現行。
- 前端維持 `pnpm lint:check`、`pnpm typecheck` 與手動 QA 清單（不新增前端測試框架）。

## Out of Scope

- 使用者介面登入認證（另行處理，見 ADR-0019）。
- 內建 TLS（以反向代理提供）。
- 代理自我更新（重跑 installer）、代理管理 UI（改名／刪除／逐台撤銷）。
- 每代理獨立憑證、代理端持久化佇列。
- IPv6 觀測、非 ARP 發現管道（ICMP、mDNS）、mirror／SPAN 全流量擷取。
- 告警／通知、自動處置（ADR-0014）。
- 統計型報表（上線率等；ADR-0016）。

## Further Notes

- **多代理同網段**：允許（皆入庫、狀態頁各列），覆蓋判定取任一在線代理；不特別支援也不視為錯誤。
- **`instance_id` 複製映像**：兩台互蓋狀態（狀態頁可見來源 IP 跳動）；v1 接受。
- **時間語意**：`last_report_at`（心跳或回報）決定在線；`last_observation_at` 是資料新鮮度；「最後可見」仍由 `last_seen_at` 決定。
- **認證碼輪替**：改後端 `.env` 與所有代理 env 後重啟兩端；README 說明。
- **伺服器同機代理**：伺服器網段要觀測時，在同一台或另一台裝代理皆可（獨立服務與使用者）。
- **建議票切**（供 `/to-tickets` 參考，非強制）：(1) migration 0008＋後端探測移除與清理任務；(2) 入庫端點＋認證＋被拒記錄；(3) 狀態端點與未觀測判定；(4) agent crate（探測移入、持續被動、週期掃描、推送、心跳）；(5) agent 安裝／移除腳本與發佈 workflow；(6) 前端（系統狀態頁、移除掃描 UI）；(7) 文件（README／`.env.example`／`install.sh` 摘要）。
