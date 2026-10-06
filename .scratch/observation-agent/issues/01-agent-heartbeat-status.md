# 01: 代理心跳與系統狀態可見

**What to build:** 觀測代理的「上線可見」最小垂直路徑：後端安裝時隨機產生的共用認證碼、代理心跳端點、代理清單與被拒回報端點，以及系統狀態頁上的兩張表。以 curl 模擬代理即可 E2E 驗證：帶正確認證碼送心跳後，系統狀態頁出現該代理（名稱、來源 IP、版本、涵蓋網段、最後回報、在線狀態）；認證碼錯誤的請求不寫入任何資料、累計到「被拒回報」表；後端未設認證碼時端點一律 503。網段以 CIDR 精確對應受管網段，對不到時代理仍可見（標「未對應」）。

**Blocked by:** None (can start immediately)

**Status:** done

- [x] migration 新增 `agent` 表（instance id、名稱、版本、來源 IP、涵蓋 CIDR、對應網段（可空、網段刪除時設空）、首次／最後回報、最後觀測）與 `agent_auth_failure` 表（來源 IP 主鍵、自報名稱／版本（可空）、首次／最後嘗試、次數）
- [x] 後端設定新增 `AGENT_AUTH_CODE`（未設定＝入庫端點 503）與 `AGENT_STALE_SECS`（預設 900，在線門檻）；`.env.example` 補說明
- [x] `POST /api/v1/agents/heartbeat`：驗證 `X-Auth-Code`、upsert 代理（來源 IP 取連線來源、忽略 XFF）、CIDR 正規化後精確對應受管網段、回 `{subnet_matched}`
- [x] 認證不符：401、不寫入、被拒回報累計（自報名稱／版本 best-effort）
- [x] `GET /api/v1/agents`（含 `online` 判定）與 `GET /api/v1/agents/auth-failures`
- [x] 系統狀態頁新增「觀測代理」與「被拒回報」兩張表（狀態 chip：在線／離線／未對應；空狀態說明）
- [x] 整合測試（HTTP＋記憶體 SQLite）：心跳往返、未對應、錯碼拒收與累計、未設碼 503、在線門檻；`pnpm lint:check`／`pnpm typecheck` 全綠

## Comments

實作完成（未 commit；依票 01 範圍，migration 0008 只新增表、不動既有表；`subnets` 移除觀測欄位留待票 08）。

- `backend/migrations/0008_agents.sql`：新增 `agent`（PK `id`＝instance_id；`name`／`version`／`source_ip`／`subnet_cidr` NOT NULL；`subnet_id` FK→`subnets(id) ON DELETE SET NULL`；`first_report_at`／`last_report_at` NOT NULL、`last_observation_at` 可空）與索引 `idx_agent_subnet_id`；新增 `agent_auth_failure`（PK `source_ip`；`claimed_name`／`claimed_version` 可空；`first_attempt_at`／`last_attempt_at` NOT NULL、`attempt_count` DEFAULT 1）。migration 0001–0007 未動。
- `backend/src/config.rs`：新增 `AGENT_AUTH_CODE`（選填；未設定或空白＝`None`）與 `AGENT_STALE_SECS`（正整數，預設 900）及單元測試 3 個（stale 驗證／預設、auth code 正規化）。
- `backend/src/agents.rs`：心跳 upsert（`first_report_at` 只寫首次）、認證失敗累計（同一來源 IP 一列；`claimed_*` 為空時保留舊值）、清單（LEFT JOIN `subnets` 取名稱；`online`＝`last_report_at ≥ now - stale_secs`，含邊界；`last_report_at` 新→舊）與被拒回報清單（`last_attempt_at` 新→舊）；在線判定單元測試 2 個。
- `backend/src/api/agents.rs`：`POST /api/v1/agents/heartbeat`（未設碼 503；`X-Auth-Code` 不符 401＋記被拒、不寫代理；通過後 400 驗證或 upsert 並回 `{subnet_matched}`；來源 IP 取 `ConnectInfo`、忽略 XFF；CIDR 以 `ipnet` 截斷 host bits 後與 `subnets.cidr` 精確比對）、`GET /api/v1/agents`（`{stale_secs, items}`）、`GET /api/v1/agents/auth-failures`。
- `backend/src/api/error.rs`：新增 `unauthorized`（401）與 `unavailable`（503）建構子。
- `backend/src/lib.rs`／`backend/src/api/mod.rs`／`backend/src/main.rs`：`AppState` 新增 `agent_auth_code`／`agent_stale_secs` 與 builder（比照 `kea`／`passive_window_secs`）；路由掛入 `/api/v1`；`main` 由 `Config` 注入。
- `frontend/src/api/agents.ts`：`Agent`／`AgentList`／`AgentAuthFailure` 型別與 `listAgents`／`listAgentAuthFailures`。
- `frontend/src/pages/KeaStatusPage.vue`：新增「觀測代理」（名稱＋instance_id、來源 IP、涵蓋網段＋對應網段名、版本、最後回報相對時間＋tooltip、狀態 chip：離線／未對應／在線）與「被拒回報」（來源 IP、自報名稱／版本、次數、最後嘗試）兩張表；空狀態說明；與 Kea／健康狀態並行載入、分區錯誤呈現；新增 `.mono-text`。
- `.env.example`：新增觀測代理區塊（`AGENT_AUTH_CODE`、`AGENT_STALE_SECS=900` 說明）。
- 測試：`backend/tests/agent_status.rs` 8 個（心跳往返＋CIDR 正規化對應與 upsert、未對應仍可見、錯碼 401＋不寫代理＋累計、解析失敗自報欄位留空、未設碼 503＋唯讀端點仍可用、在線門檻與排序（直接改 DB 時間）、輸入驗證 400、網段刪除 FK 設空）。
- 驗收：`cargo test` 全綠（327 passed、0 failed、7 ignored）；`cargo fmt --check` 綠；`cargo clippy` 新檔案無警告；`pnpm lint:check`、`pnpm --filter frontend typecheck` 全綠；另以短命服務＋curl 實測心跳往返（含 CIDR 正規化、錯碼 401 與被拒累計）。
- 備註：migration 編號 0008 依票指示「只新增表」；spec 所述 `subnets` 移除觀測欄位、`AGENT_AUTH_FAILURE_RETENTION_DAYS` 清理與 `POST /agents/observations` 屬票 08／02，未實作。狀態 chip 以「離線」優先於「未對應」（未回報時代理是否對應不影響失聯判讀）。
