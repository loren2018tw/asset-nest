# 規格：IP 觀測（可見性與變動歷史）

- 狀態：ready-for-agent（2026-10-06）
- 詞彙依 `GLOSSARY.md`「觀測詞彙」；決策見 `docs/adr/0014`（觀測與宣告分離）、`docs/adr/0015`（本地優先的探測策略）、`docs/adr/0016`（觀測記錄模型）。
- 兩層驗證分界沿用 `docs/adr/0006`；Kea 整合沿用 `docs/adr/0002`、`0010`、`0011`。

## Problem Statement

管理者目前無法回答「這個已指派位址現在還在不在用」：Assignment 只記現況、不留歷程，Kea 租約頁只有當下動態租約、也不落庫。於是——

- 已停用的個人裝置或非管理的設備不敢刪除，位址無法安全釋出；反過來，真正還在使用的位址可能被誤回收。
- 靜態設定（非 DHCP）的設備完全沒有痕跡；「某台設備換過哪些 IP」「哪個 IP 被不同 MAC 輪流使用」只能靠記憶或事後翻帳。
- 想找未登錄的陌生設備（未指派位址有主、未在資產庫的 MAC），沒有任何管道。
- ICMP 對 Windows 常有盲點、被動讀 ARP 表覆蓋率低、/16 又不能整段亂掃——需要一套有界限、低負擔的觀測機制。

## Solution

系統定期對「本機同 L2」的網段做網路觀測（ARP 探測為主、Kea 租約為輔），把結果記成兩層資料：每個位址的**現況**（最後可見時間、最後 MAC、來源、最後檢查時間）與**變化事件**（首見、MAC 變更）。觀測是唯讀的影子層，永遠不修改宣告資料。

使用者在既有畫面上看到：

- IP 清單與資產清單多一欄「最後可見」（N 天前／從未上線／未觀測，可排序），用來判斷哪些位址疑似可回收。
- IP 詳情的觀測歷史時間軸，以及可切換的 MAC 視角（這個 MAC 用過哪些位址）；未知 MAC 標示「未登錄」。
- 篩選「非法佔用 IP」「有未登錄 MAC」，與兩個新衝突標記（`ObservedMacMismatch`、`ObservedOnUnassigned`）。
- 每個網段可開關觀測、開關探索掃描並設定間隔；可手動「立即掃描」。
- 刪除資產／取消指派的確認框顯示最後可見資訊，避免誤刪。

## User Stories

**觀測設定與掃描**

1. 作為管理者，我想在每個 IPv4 網段開啟或關閉觀測，以便只掃描我在意的網段。
2. 作為管理者，我想在已開啟觀測的網段另外開啟「探索掃描」並設定間隔（預設每日），以便用低頻率找出未知設備。
3. 作為管理者，我想在網段表單看到「本機是否同 L2」與上次探索時間，以便知道哪些網段實際上觀測得到。
4. 作為管理者，我想在 IP 清單與網段表單按「立即快速掃描」或「立即探索掃描」，以便當下刷新資料。
5. 作為管理者，我不想整段 /16 每 15 分鐘被打擾——快速掃描只涵蓋已指派與有租約的位址。
6. 作為管理者，我想探索掃描離峰、限速、一次只跑一個網段，以免影響網路。
7. 作為管理者，我想在非本機同 L2 的網段開啟觀測時看到提示「v1 無法觀測」，而不是默默沒有資料。

**最後可見與判讀**

8. 作為管理者，我想在 IP 清單看到「最後可見」欄（N 天前／從未上線／未觀測），以便判斷位址是否還在使用。
9. 作為管理者，我想該欄可排序，以便把最久沒出現的位址排在最前面。
10. 作為管理者，我想在該欄的提示中看到來源（ARP 或 Kea 租約）與最後檢查時間，以便判斷資料可信度。
11. 作為管理者，我想在資產清單看到該資產的「最後可見」並可排序，以便找出疑似停用的資產。
12. 作為管理者，我想在資產詳情看到最後可見，並從其介面 MAC 點開觀測歷史，以便逐一檢查。
13. 作為管理者，我想在刪除資產的確認框看到最後可見與最後 MAC，以免刪掉其實還在線的資產。
14. 作為管理者，我想在取消指派／刪除保留的確認框看到該位址的最後可見，以免誤釋出使用中的位址。
15. 作為管理者，我想清楚區分「未觀測」（沒開啟或非本機同 L2）與「從未上線」（有檢查但從未看到），以免把沒看過誤讀成不在。

**歷史查詢**

16. 作為管理者，我想在 IP 詳情看到事件時間軸（首見、MAC 變更），以便回溯這個位址發生過什麼。
17. 作為管理者，我想看到「這個位址被哪些 MAC 用過」的清單，含各 MAC 的首見／最後可見與來源。
18. 作為管理者，我想看到「這個 MAC 用過哪些位址」的清單，以便回答「某台設備的 IP 是否常變動」。
19. 作為管理者，我想在歷史畫面只看到變化事件（而非每輪樣本），以便查詢乾淨快速。
20. 作為管理者，我想把單一 IP 的觀測歷史匯出 CSV，以便貼進報告或存查。
21. 作為管理者，我想未知 MAC 顯示「未登錄」、已知 MAC 能連到對應資產，以便快速歸屬。

**衝突與篩選**

22. 作為管理者，我想在 IP 清單篩選「非法佔用 IP」，以便找出被佔用的非指派位址。
23. 作為管理者，我想在 IP 清單篩選「有未登錄 MAC」，以便找出不在資產庫的設備。
24. 作為管理者，我想在已指派位址看到 `ObservedMacMismatch` 標記（宣告 MAC 與觀測不符），以便發現換了網卡或佔用情形。
25. 作為管理者，我想在未指派且非池內位址看到 `ObservedOnUnassigned` 標記（UI 顯示「非法佔用 IP」），以便察覺異常。
26. 作為管理者，我不希望池內位址被 DHCP 正常使用時被標成衝突。

**保留與資料安全**

27. 作為管理者，我想設定事件保留期（預設一年），到期自動清理，不影響現況。
28. 作為管理者，我想在清除觀測資料後，宣告（指派／保留）完全不受影響。
29. 作為管理者，我想已指派但從未上線的位址仍被持續檢查（最後檢查時間持續更新），而不是被忽略。

## Implementation Decisions

### 資料庫（migration 0006）

- `subnets` 新增欄位：
  - `observed INTEGER NOT NULL DEFAULT 0`（快速掃描開關；v6 網段不得為 1）。
  - `discovery_enabled INTEGER NOT NULL DEFAULT 0`（探索掃描開關；需 `observed=1`）。
  - `discovery_interval_minutes INTEGER NULL`（NULL＝用全站預設）。
  - `last_discovery_at TEXT NULL`。
- 新增 `ip_presence`（現況；主鍵對齊 `ip_assignments` 的鍵）：
  - `subnet_id`（FK `subnets`，`ON DELETE CASCADE`）、`address TEXT`。
  - `last_seen_at TEXT NULL`、`last_seen_mac TEXT NULL`、`last_seen_source TEXT NULL`（`arp`／`kea_lease`；未來 `agent`）、`last_checked_at TEXT NULL`。
- 新增 `observation_event`（append-only，只在變化時寫）：
  - `id`、`subnet_id`（FK，`ON DELETE CASCADE`）、`address`、`mac TEXT NULL`、`kind TEXT`（`first_seen`／`mac_changed`）、`source TEXT`、`observed_at TEXT NOT NULL`。
  - 索引：`(subnet_id, address, observed_at)`、`(mac, observed_at)`、`(observed_at)`（清理用）。
- 時間格式沿用既有 `YYYY-MM-DDTHH:MM:SSZ`（UTC）。
- 現況列由**掃描產生**（不掛在指派生命週期上）：指派位址即使未回應也會被 upsert `last_checked_at`；未指派位址只在被看見時建列；取消指派不刪列（歷史保留）。

### 探測邊界（唯一新 seam）

- 新模組 `probe`（crate 內），定義 `Prober`：
  - `is_local(subnet) -> bool`：本機是否有介面位址落在該 v4 子網（判定同 L2）。
  - `probe(subnet, targets) -> Vec<(Ipv4Addr, Mac)>`：對目標送 ARP 請求、收集回應（回覆窗約 2 秒，實作可調）。
- 實作：
  - **raw 模式（預設）**：Linux `AF_PACKET` raw socket 自建／解析 ARP 框架；介面列舉用 `getifaddrs`（`libc`）。不依賴 libpcap。
  - **unprivileged 降級模式**：對目標丟 UDP 觸發 kernel ARP 解析，再讀 `/proc/net/arp`（重用 `peer` 模組的解析）。
  - `OBSERVATION_PROBE_MODE=auto|raw|unprivileged`（預設 `auto`：raw 遇權限錯誤時降級並記錄一次警告）。
- `Prober` 以 `Arc<dyn Prober + Send + Sync>` 掛在 `AppState`；測試注入 stub。

### 掃描服務（可直接呼叫，排程只是薄迴圈）

- `run_quick(subnet_id, now)`：
  - 前提：`observed`、v4、`prober.is_local`。
  - 目標集合＝該網段已指派位址 ∪ Kea 目前有效（`state=default`）租約位址（以 `kea_subnet_id` 對應；Kea 未設定時略過）。
  - Kea 租約以 `cltt` 記為 seen（來源 `kea_lease`）；ARP 回應記為 seen（來源 `arp`，時間 `now`）；同一位址的 `last_seen_*` 取最近者。
  - 所有目標（含未回應者）upsert 現況列的 `last_checked_at`。
  - 寫事件：目標位址第一次有 `last_seen_mac` → `first_seen`；`last_seen_mac` 改變 → `mac_changed`；MAC 相同只更新時間、不寫事件。
- `run_discovery(subnet_id, now)`：
  - 前提：`observed`＋`discovery_enabled`＋v4＋`is_local`。
  - 目標＝該網段全部 host 位址（沿用 `HostRange`）；依 `OBSERVATION_DISCOVERY_RATE_PPS`（預設 1000）限速發送後收集回應。
  - 處理同 `run_quick`；另更新 `subnets.last_discovery_at`。
- `cleanup_events(retention_days, now)`：刪除 `observed_at` 早於保留期的 `observation_event`；不動 `ip_presence`。
- 排程器（`main` 內）：每 60 秒 tick 一次；快速掃描每網段 15 分鐘（常數）；探索掃描依網段覆寫或 `OBSERVATION_DISCOVERY_INTERVAL_SECS`（預設 86400）；同時只執行一個掃描；啟動後無上次紀錄者視為 due。保留清理每日執行一次。

### 環境設定

- `OBSERVATION_PROBE_MODE`（`auto|raw|unprivileged`，預設 `auto`）。
- `OBSERVATION_DISCOVERY_INTERVAL_SECS`（預設 86400）。
- `OBSERVATION_DISCOVERY_RATE_PPS`（預設 1000）。
- `OBSERVATION_RETENTION_DAYS`（預設 365）。
- 快速掃描間隔固定 15 分鐘（不入設定）。

### 讀取端（即時計算、不落地）

- 「未觀測」＝該網段 `observed=0` 或本機非同 L2（`is_local=false`）；v6 恆為未觀測。
- IP 詳情／清單新增：`last_seen_at`、`last_seen_mac`、`last_seen_source`、`last_checked_at`（清單僅需要者）、`observed`（有效涵蓋）。
- IP 清單排序白名單新增 `last_seen`（NULL――含未指派與從未上線――固定排最後，不分升降冪）。
- IP 清單篩選新增 `observed=unassigned_seen|unknown_mac`：
  - `unassigned_seen`：無指派且非池內、但 `last_seen_mac` 非空。
  - `unknown_mac`：`last_seen_mac` 不在任何 Interface 的 MAC 集合（不分大小寫）。
- 衝突即時計算（沿用 `conflicts` 慣例），`conflicts` 陣列新增：
  - `ObservedMacMismatch`：已指派位址、宣告介面有 MAC、且 `last_seen_mac` 與其不同。
  - `ObservedOnUnassigned`：同 `unassigned_seen` 條件。
  - 網段列表的「衝突數」語意不變（僅指派語意衝突），觀測標記只在 IP 列呈現。
- 資產清單／詳情新增 `last_seen_at`：取其任一介面之已指派位址、或其介面 MAC 命中（含未指派位址）的現況列最大值；NULL 顯示「—」。資產清單排序白名單新增 `last_seen`。
- MAC 歷史彙總（`GET /api/v1/observations/mac/{mac}`）：將事件／現況按 MAC 聚合出視覺用的 sightings（首位址首見、最後可見、來源）。

### HTTP API

- `PATCH /api/v1/subnets/{id}` 擴充：`observed`、`discovery_enabled`、`discovery_interval_minutes`（可清除）。
  - 結構驗證：v6 不可開；`discovery_enabled=1` 需 `observed=1`；間隔需為正整數。
- 網段回應（詳情與列表摘要）新增：`observed`、`discovery_enabled`、`discovery_interval_minutes`、`last_discovery_at`、`local`（`is_local`；摘要僅 `observed`／`local`）。
- 新增 `POST /api/v1/subnets/{id}/sweeps`，body `{"mode":"quick"|"discovery"}`；同步執行後回摘要 `{mode, targets, seen, duration_ms}`（discovery 另回 `last_discovery_at`）。錯誤：未開觀測、探索未開、非同 L2、v6 → 400。
- 新增 `GET /api/v1/subnets/{id}/ips/{address}/observations`：現況＋事件清單（時間新→舊）。
- 新增 `GET /api/v1/subnets/{id}/ips/{address}/observations/export`：CSV（欄位 `address, mac, kind, source, observed_at`；檔名沿用 `encode_filename` 慣例）。
- `GET /api/v1/subnets/{id}/ips` 與 `GET /api/v1/assets`：加入上述欄位、排序與篩選。

### 前端

- IP 清單：新增「最後可見」欄（可排序、tooltip 顯示來源與最後檢查時間）、觀測篩選下拉、「觀測」列操作開啟歷史對話框、工具列「立即掃描」（快速；探索開啟時提供探索選項）；衝突徽章新增兩個代碼與說明。
- 新增觀測歷史對話框元件：支援「以 IP 進入」與「以 MAC 進入」兩種模式；顯示現況摘要、事件時間軸、用過的 MAC／位址清單（未知 MAC 標「未登錄」、已知可連資產）；含 CSV 匯出按鈕。
- 資產清單：新增「最後可見」欄（可排序）；資產詳情顯示最後可見，介面 MAC 可點開歷史對話框。
- 刪除資產、取消指派／刪除保留的確認框：顯示對應的最後可見與最後 MAC（僅提示）。
- 網段表單：新增觀測區塊（觀測開關；開啟且同 L2 時顯示探索開關、間隔、上次探索時間與立即掃描；非本機同 L2 顯示提示；v6 隱藏）。
- 新增相對時間工具（N 天前／N 小時前／剛看到）。

### 部署

- `deploy/install.sh` 的 systemd 單元加入 `AmbientCapabilities=CAP_NET_RAW` 與 `CapabilityBoundingSet=CAP_NET_RAW`（保留 `NoNewPrivileges`）；重跑安裝即更新。
- `.env.example` 與 `README` 補充新環境變數與觀測功能說明（含「僅本機同 L2、未開＝未觀測」）。

## Testing Decisions

- 原則：只測外部行為（HTTP 回應與可觀察的資料效果），不測實作細節；優先用 repo 既有最高縫——`app(AppState)`＋記憶體 SQLite（套 migrations）＋`tower::ServiceExt::oneshot`。
- 唯一新 seam 是 `Prober`：測試注入 stub（可腳本化回應、記錄收到的目標集合與本地判定），任何碰真實網路的行為不進自動測試。
- 既有 seam 重用：Kea 以本機 stub HTTP 伺服器（如同 `backend/tests/kea_view.rs`）；時間一律以參數注入（`now`），測試用固定時間戳。
- 規劃中的測試：
  - `tests/observation_sweep.rs`：快速掃描只探「已指派＋有效租約」；ARP／租約 seen 的來源與最近者語意；重複掃描同 MAC 不重複寫事件；MAC 變更寫 `mac_changed`；未回應的指派位址更新 `last_checked_at` 且不動 `last_seen_at`；探索掃描涵蓋全 host 範圍並更新 `last_discovery_at`；發現未知設備。
  - `tests/observation_settings.rs`：網段 PATCH 的結構驗證（v6 不可開、探索需觀測、間隔合法性）、`local` 標示（stub 決定）、`last_discovery_at` 往返。
  - `tests/observation_history.rs`：IP 歷史端點、MAC 歷史端點（已知／未知 MAC）、CSV 匯出格式、事件排序；資產 `last_seen` 聚合。
  - `tests/ips.rs`（擴充）：`sort=last_seen`（NULL 最後）、`observed` 篩選、兩個新衝突碼。
  - 單元測試：ARP 回應解析、事件轉移的純邏輯、清理函式（service 層＋SQLite）。
  - 真機（`#[ignore]`，比照 `tests/kea_connectivity.rs`）：raw ARP 於實際 LAN 的行為驗證。
- 前端：維持現狀——`pnpm lint:check`、`pnpm typecheck` 與手動 QA 清單（不新增前端測試框架）。

## Out of Scope

- 跨 VLAN／遠端 agent：本階段只做本機同 L2；資料層保留來源欄位與入庫邊界，agent 實作與其 API 認證另案。
- IPv6 觀測（v6 網段不可開啟觀測）。
- 告警／通知（只標記與顯示）。
- 自動刪除、自動釋出、以觀測覆寫宣告（ADR-0014）。
- 上線率百分比等全量樣本統計（ADR-0016）。
- 租約對帳（既有後續階段）與 IPv6 租約。
- IP 清單整批匯出、整網段觀測報表（僅提供單一 IP 歷史 CSV）。
- 前端自動化測試。
- Kea 未設定時的租約來源（僅 ARP 探測）。

## Further Notes

- **三態定義**：`未觀測`＝所屬網段未開觀測或本機非同 L2；`從未上線`＝有效涵蓋下、現況列 `last_seen_at` 為空（含尚未首次掃描的短暫空窗，開啟後下一 tick 內即掃描）；`N 天前`＝其餘。
- **租約語意的限制**：`kea_lease` 來源以 `cltt` 為時間，最多可能落後半個租期；因此「最後可見」欄的 tooltip 必須顯示來源，讓使用者分辨「ARP 直接看到」與「租約活動」。
- **MAC 正規化**：寫入前一律正規化為小寫冒號格式（沿用 Interface 的規則），比較時不分大小寫。
- **負擔**：快速掃描目標小（指派＋租約）可一次送出後等回覆窗；探索掃描才需限速。`/16` 全掃以 1000 pps 約 65 秒，一次僅跑一個網段。
- **事件語意**：`first_seen` 為「該位址第一次被看到」；`mac_changed` 為「同一位址換 MAC」；舊 MAC 的離場時間由下一次變化推得，不另寫事件。
- **建議票切**（供 `/to-tickets` 參考，非強制）：(1) migration 與觀測寫入 domain；(2) `Prober` seam＋raw／降級實作＋排程器＋deploy／env；(3) 掃描與設定端點；(4) 讀取端（清單欄位／排序／篩選／衝突／歷史／匯出）；(5) 前端（欄位、篩選、歷史對話框、網段表單、確認框）；(6) 文件（README／`.env.example`）。
