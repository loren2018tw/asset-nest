# 02: 掃描觀測入庫（sweep）

**What to build:** 代理可把主動掃描結果送回後端並反映在既有介面。`POST /api/v1/agents/observations` 接受 sweep 報告（檢查過的位址與有回應的位址＋MAC、`observed_at`），寫入既有觀測影子層：檢查過的更新「最後檢查」、有回應的更新「最後可見」並推導事件（首見／MAC 變更、latest-wins）；IP 清單與詳情（最後可見欄、歷史時間軸、衝突標記）不需新前端即時反映。時間規則：未來值夾到現在、過去照收（離線補送）、依時間排序套用。本票同時把「未觀測」判定擴充為「有在線代理涵蓋」（與既有本機判定並存，票 08 收斂單軌）。

**Blocked by:** 01

**Status:** done

- [x] `POST /api/v1/agents/observations`（sweep 報告）：`checked` 更新最後檢查；`seen` 更新最後可見（來源 `arp`）與 MAC
- [x] 事件推導照舊：首見寫 `first_seen`、MAC 不同寫 `mac_changed`、同 MAC 不重複寫；重複回報不重複事件
- [x] 網段 CIDR 精確對應；對不到回 `{stored:false, reason:"subnet_unmatched"}`、不入庫、代理狀態照記
- [x] 認證與未設碼行為沿用票 01（401／503）
- [x] 時間：未來 `observed_at` 夾到現在；過去照收；多報告依 `observed_at` 排序後套用；更新最後觀測時間
- [x] 「未觀測」判定併入「有在線代理涵蓋」（與既有本機涵蓋並存）
- [x] 整合測試：寫入、事件轉移、排序與夾制、未對應、未觀測判定；`pnpm lint:check`／`pnpm typecheck` 全綠

## Comments

實作完成（未 commit；依票 02 範圍，僅動 sweep 入庫與讀取端涵蓋判定；passive 留票 03、前端文案留票 06、單軌收斂留票 08）。

- `backend/src/api/agents.rs`：新增 `POST /api/v1/agents/observations`（body `{instance_id,name,version,subnet_cidr,reports:[{kind,observed_at,checked[],seen[{address,mac}]}]}`）。認證抽出 `authorize`（心跳與回報共用）：未設碼 503、錯碼 401＋被拒回報累計（自報名稱／版本 best-effort，沿用票 01）。`kind` 僅接受 `sweep`（其他／缺漏回 400 `reports[i].kind`）；`observed_at` 嚴格 `YYYY-MM-DDTHH:MM:SSZ`；位址須 IPv4、MAC 以 `normalize_mac` 驗證並正規化（400 標示 `reports[i].checked[j]`／`seen[j].address|mac`）。CIDR 以 `find_subnet_id` 正規化精確對應；對不到回 `{stored:false, reason:"subnet_unmatched"}` 且只以 `record_heartbeat` 記代理狀態（算一次回報）。
- `backend/src/agents.rs`：`record_heartbeat` 抽出共用 `upsert_agent`（心跳與回報皆更新 `last_report_at`）。新增 `SweepReport`／`ObservationReport` 與 `record_observations`：單一交易內 upsert 代理→報告依 `observed_at` 穩定排序舊到新→未來值 `min(now)` 夾制→`checked` 逐筆 `upsert_checked`、`seen` 逐筆 `record_seen`（來源 `arp`、時間為報告時間）→成功後寫 `last_observation_at = now`。新增 `subnet_has_online_agent`（`last_report_at` 在 `AGENT_STALE_SECS` 內；門檻語意同 `is_online`，極端值視為在線）。
- `backend/src/observation.rs`：`upsert_checked` 改 `pub(crate)` 並加 latest-wins 防護（`last_checked_at` 為 NULL 或 `excluded >= 現值` 才更新），使離線補送的較舊報告不覆寫較新檢查時間；新增 `effective_coverage`＝v4 ∧（既有「`subnet.observed` ∧ `prober.is_local`」或「該網段有在線代理」），`ip_history` 改以同一函式計算 `observed`（新增 `now`／`stale_secs` 參數）；`ObservationView` 與相關註解同步更新。
- `backend/src/api/ips.rs`：IP 清單的有效涵蓋改呼叫 `observation::effective_coverage`（不再直接以 `observed ∧ is_local` 推算）；`backend/src/api/observations.rs`：IP 歷史／匯出帶入 `now` 與 `agent_stale_secs`。資產彙總只讀 `last_seen_at`，不涉涵蓋判定，未動。
- `backend/tests/agent_ingest.rs`：新增 8 個整合測試（沿用 `observation_settings.rs` 的 stub 探測基建＋`agent_status.rs` 的 `ConnectInfo` 注入）：sweep 寫入（checked／seen／來源／MAC 正規化／代理在線與最後觀測）、同 MAC 重複與 MAC 變更事件恰一次、亂序報告排序＋未來夾制＋較舊補送 latest-wins、未對應只記代理狀態、錯碼 401／未設碼 503 與被拒累計、passive 尚未支援 400、輸入驗證 400（10 例）、在線代理使清單／詳情「已觀測」且代理過期回到未觀測（資料保留）。
- 驗收：`cargo test` 全綠（335 passed、0 failed、7 ignored，含新增 8 個）；`cargo fmt --check` 綠；`cargo clippy` 新檔案無警告；`pnpm lint:check`、`pnpm --filter frontend typecheck` 全綠。
- 備註：`observed_at` 採嚴格固定格式（僅 `Z`、無小數秒），與資料庫時間字串比較一致；`last_observation_at` 記寫入當下（`now`）而非報告時間，維持「最後一次回報」語意。前端「未觀測」提示文案仍以舊 `observed`／`local` 欄位生成（後端回傳的 `observed` 已正確反映代理涵蓋），依規格留待票 06 更新；掃描前提（`validate_quick`／`validate_discovery`）與排程器仍用舊判定，非讀取端分類，留待票 08 收斂。
