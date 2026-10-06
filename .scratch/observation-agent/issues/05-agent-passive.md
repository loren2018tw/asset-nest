# 05: 觀測代理持續被動監聽

**What to build:** 代理常駐被動監聽 ARP（只收不送）：解析 sender、排除 0.0.0.0／multicast／自身；記憶體聚合（位址→最新 MAC），每 30 秒 flush 有更新者、以 flush 時間為觀測時間推送 passive 報告。與主動掃描並行、互不阻塞。

**Blocked by:** 03, 04

**Status:** done

- [x] 被動監聽只收不送；sender 解析與排除規則單元測試
- [x] 記憶體聚合去重；每 30 秒只送有更新者；flush 失敗進佇列走既有退避
- [x] 真代理運行時，「網段外觀測」出現同 L2 但 CIDR 外的設備
- [x] 與掃描並行不互相阻塞（同時被動＋主動的驗證）
- [x] 真機 `#[ignore]` 被動測試移入代理並可執行

## Comments

實作完成（未 commit；僅動 `agent/` crate；後端與其他票未動）。

- `agent/src/probe.rs`：新增持續被動監聽 [`passive_listen`]（`AF_PACKET`／`ETH_P_ARP` **只收不送**；每筆合法 sender 呼叫 `on_sender`，`should_stop` 每輪（單次 `recv` 等待上限 200ms）檢查一次）與短窗 `passive_observe`（窗長 0 不開 socket；供真機測試）。sender 解析（opcode 1／2）與排除規則（`0.0.0.0`、multicast／有限與網段廣播、本機介面自身位址與 MAC〔不分大小寫〕）沿用票 04 自後端移入的純函式 `parse_arp_sender`／`valid_passive_sender`；介面與 MAC 由 `local_interface`／`read_interface_mac` 取得。新增零權限／無介面短路單元測試。
- `agent/src/passive.rs`（新）：`Aggregator`＝位址→最新 MAC（`HashMap`）＋「自上輪 flush 後有更新」位址集（`HashSet`）；重複 sender 只更新、不重複輸出。`FLUSH_INTERVAL`＝30 秒；`flush(observed_at)` 取出有更新者（位址遞增）產生 `PassiveReport`（以 flush 時間為 `observed_at`）、清除標記；本輪無更新回 `None`（不送）。
- `agent/src/push.rs`：新增 `PassiveReport` 與通用 `Report` enum（`Sweep`／`Passive`）；`PushQueue`、`split_report`（≤5000 筆切批）與 `try_flush` 改收通用報告，passive 切批依 `senders`。請求 body 為 `{kind:"passive", observed_at, senders:[{address, mac}]}`；sweep 序列化欄位與先前完全相同。佇列容量、退避 5s→5min、401 保留、`subnet_unmatched` 丟棄等失敗處理完全不變（passive flush 只負責入列＋喚醒）。
- `agent/src/runner.rs`：新增兩個任務——被動監聽（`spawn_blocking`，停止旗標由 Ctrl-C 設定，收尾等待 ≤200ms）與 flush 迴圈（首次在 30 秒後，之後每 30 秒；無更新不 push）。掃描、推送、心跳照舊，四任務並行互不阻塞；監聽不可用（缺 CAP_NET_RAW 等）記一次警告後結束，其餘照常。
- 測試：單元 45（既有 36＋新增：零窗不開 socket、無介面短路、聚合去重取最新 MAC、只 flush 有更新者、flush 後標記清除、同 MAC 再觀測算更新、passive 切批、佇列收 passive 並計筆數、flush 時間格式與 30 秒常數）；整合 10（既有 8＋新增 2，沿用 stub server）：passive 報告形狀與 `X-Auth-Code` 標頭（`kind:"passive"`、`observed_at`＝flush 時間、`senders` 內容、不帶 sweep 欄位）、每輪 flush 只含有更新者；真機 `#[ignore]` 3（新增 `tests/live_passive.rs` 2：唯讀短窗被動監聽、被動監聽與主動探測同時執行；既有 `live_probe.rs` 1）。
- 驗收：`pnpm test:agent` 全綠（45＋10 passed、3 ignored）；`cargo fmt --manifest-path agent/Cargo.toml -- --check` 綠；`cargo clippy --manifest-path agent/Cargo.toml --all-targets` 無警告；`cargo test --manifest-path backend/Cargo.toml` 未破壞（各檔全綠）；`pnpm lint:check`、`pnpm --filter frontend typecheck` 全綠。
- 真機限制：本實作環境無 CAP_NET_RAW 與同 L2 網路，`#[ignore]` 真機測試可建置並執行至權限檢查（實測回「需要 CAP_NET_RAW 權限」），**尚未**實收 sender、實測「與掃描並行」與「網段外設備出現在網段外觀測清單」；待有權限／真實網段的機器執行 `cargo test --manifest-path agent/Cargo.toml --test live_passive -- --ignored --nocapture`。
- 備註／決策：sweep 與 passive 共用同一推送佇列與退避（票面要求「以既有推佇列入列」），故容量 10000 筆與失敗重試為兩者共享；flush 首次在 30 秒後（避免啟動空 flush）；同一位址在同一窗重複出現只算一次、flush 後再觀測即再算更新；被動只報合法 sender，CIDR 內丟棄由後端負責（同 ADR-0017／票 03）。
