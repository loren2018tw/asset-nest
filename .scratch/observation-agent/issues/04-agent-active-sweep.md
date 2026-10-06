# 04: 觀測代理本體（主動掃描）

**What to build:** 全新的觀測代理程式（獨立 crate、Linux）：讀取設定（伺服器位址、認證碼、網段（自動偵測或明確指定）、名稱、掃描間隔與速率、instance id）；以 raw socket 對網段全部 host 位址限速主動探測（ARP）；週期執行並將結果批次推送後端（佇列上限、失敗退避、401 明確處理）；每 60 秒心跳。探測實作自後端移入（後端本體仍保留至票 08）。在本機執行代理即可在既有介面看到真實觀測資料。

**Blocked by:** 02

**Status:** done

- [x] 新 crate 可獨立建置並含單元測試（ARP 組框／解析、sender 過濾、CIDR host 列舉、批次切分）
- [x] 設定與預設：掃描間隔 900 秒、速率 1000 pps、名稱預設 hostname；網段空＝自動偵測（多候選介面時拒絕啟動並提示）
- [x] 週期掃描：全段 host（扣 network／broadcast；/31、/32 全列）限速送收；回報 checked／seen
- [x] 推送：帶認證碼標頭；每請求 ≤ 5000 筆拆批；失敗退避 5s→5min；佇列上限 10000 筆（滿載丟最舊＋警告）；401 記明確錯誤不熱迴圈
- [x] 心跳每 60 秒；未對應回應記一次警告
- [x] 整合測試（stub server）驗證推送與退避；真機 raw 探測 `#[ignore]` 比照現行
- [x] 代理測試可一鍵執行（等比 `pnpm test:agent`）；`cargo fmt --check` 全綠

## Comments

實作完成（未 commit；僅新增 `agent/` crate 與 `package.json`／`.gitignore` 各一行；未改動 `backend/` 任何既有行為）。

- 新獨立 crate `agent/`（edition 2024、binary `asset-nest-agent`、lib `asset_nest_agent` 供整合測試呼叫）：`Cargo.toml` 依賴比照後端精簡（anyhow／chrono／ipnet／libc／reqwest(json+rustls)／serde／serde_json／tokio／tracing／tracing-subscriber）；`Cargo.lock` 已產生（後端慣例為入庫追蹤，本票未 commit，建議一併納入）。
- `agent/src/config.rs`：`AGENT_SERVER_URL`／`AGENT_AUTH_CODE`／`AGENT_INSTANCE_ID` 必填（錯誤訊息指明變數）；`AGENT_SUBNET_CIDR` 空＝自 `getifaddrs` 列舉非 loopback、IFF_UP 介面的網段（位址＋netmask 推導、去重），唯一候選才採用，多候選或找不到皆拒絕啟動並列出候選與提示；解析後再驗證網段內有本機介面位址（否則拒絕並說明）。`AGENT_NAME` 預設 `gethostname`、`AGENT_SWEEP_INTERVAL_SECS` 預設 900、`AGENT_SWEEP_RATE_PPS` 預設 1000（0／非數字拒絕）。
- `agent/src/probe.rs`：自 `backend/src/probe.rs` 複製 raw `AF_PACKET`／`ETH_P_ARP` 組框與解析（`build_arp_request`／`parse_arp_reply`／`parse_arp_sender`／`valid_passive_sender`／`normalize_mac`）、`getifaddrs` 介面列舉與限速批次送收（每批最多 `rate_pps`、批距約 1 秒、回覆窗 2 秒／閒置 500ms 提早結束）；後端本體保留不動。被動監聽迴圈屬票 05，僅先移入純函式並附單元測試。另有 `raw_available` 啟動能力檢查（失敗只警告不阻擋，提示缺 CAP_NET_RAW）。
- `agent/src/host_range.rs`：`HostRange` 自 `ips.rs` 複製（扣 network／broadcast；/31、/32 全列）。
- `agent/src/push.rs`：請求 body `{instance_id,name,version,subnet_cidr,reports:[{kind:"sweep",observed_at,checked,seen}]}`＋`X-Auth-Code`；`split_report` 於入列時切為每筆 ≤ 5000 筆；`PushQueue`（容量 10000 筆）滿載丟最舊並記警告；`Backoff` 5s→5min 指數（成功歸零）；`try_flush` 分類後端回應——`stored:true` 移除、`subnet_unmatched` 警告＋丟棄、401 記 error 並保留批次、連線／5xx 保留批次，兩者由 runner 依退避等待（不熱迴圈）。
- `agent/src/heartbeat.rs`＋`runner.rs`：心跳啟動後立即一次、之後每 60 秒；`subnet_matched:false` 以 `MatchTracker` 只在狀態變化時警告（恢復對應記 info）。掃描用 `interval_at(now+interval)`，**首次在間隔後**（啟動不掃描；CI 大間隔即可）；掃描失敗記警告並略過本輪。
- 測試：單元 36（ARP 版面／round-trip／畸形框架、sender 過濾、host 列舉、切批 ≤5000、佇列滿丟最舊、peek 打包、退避序列、心跳狀態機、設定解析與預設）；整合 8（自建 `TcpListener` 最小 stub，不新增依賴）：心跳 body／標頭、sweep 報告形狀、5001 筆拆兩請求、500/503 退避後重試成功、401 保留批次且結果明確、`subnet_unmatched` 丟棄後不重送、連線錯誤保留批次；真機 `#[ignore]` 1（`tests/live_probe.rs`，唯讀送 gateway＋首 host）。
- 驗收：`pnpm test:agent` 全綠（36＋8 passed、1 ignored）；`cargo fmt --manifest-path agent/Cargo.toml -- --check` 綠；`cargo clippy --all-targets` 無警告；`cargo test --manifest-path backend/Cargo.toml` 未破壞（各測試檔全綠）；`pnpm lint:check`、`pnpm --filter frontend typecheck` 全綠。
- 備註／決策：整合測試以手寫最小 HTTP stub 驗證連線行為（不發真實探測）；啟動時多做一次非致命的 raw 能力檢查（票面僅要求網段驗證，此為部署提示）；401 時批次保留（待修正認證碼後重送，符合「照退避」語意）；`.gitignore` 增加 `agent/target/`。
