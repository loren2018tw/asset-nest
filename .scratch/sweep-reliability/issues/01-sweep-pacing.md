# 01: 掃描逐筆節流與送訊中收訊

**What to build:** 主動掃描從「批次爆量＋每批 2 秒回覆窗」改為逐筆節流、送訊期間即時收訊；`AGENT_SWEEP_RATE_PPS` 成為真正的每秒送訊上限；收尾回覆窗的閒置判斷改以「回覆」為準（背景 ARP 不延長）。預設速率 1000 → 200（config、install 腳本、README 同步）。

**Blocked by:** 無

**Status:** done

- [x] `probe_targets_platform`：逐筆 `send_frame`＋每筆後非阻塞 drain（`MSG_DONTWAIT`）＋deadline 制節流（間隔 `1/rate` 秒）
- [x] `drain_window`：全數已回應／距最後一筆回覆 500ms／2 秒硬上限即結束；批次概念退場（`collect_replies`、`batch_pacing_delay` 移除；`reply_window_done` 保留、語意更新）
- [x] 收集條件不變（opcode 2、sender ∈ 本輪目標、每址第一筆；成員判定 `HashSet`）
- [x] 預設速率 1000 → 200（`config.rs`、`deploy/agent-install.sh`、`README.md`）
- [x] 單元測試：`frame_interval`、`accept_reply`（linux）、`reply_window_done` 語意更新

## Comments

實作於 `cb8d72c`（與票 02 同 commit）：

- `probe.rs`：`probe_targets_platform` 改為逐筆節流（`frame_interval`，deadline 制）＋送訊中非阻塞收訊（`drain_available`／`accept_reply`）＋收尾回覆窗（`drain_window`；`reply_window_done` 參數更名 `since_last_reply`）。`collect_replies`、`batch_pacing_delay` 移除。
- 預設速率：`DEFAULT_SWEEP_RATE_PPS = 200`；`deploy/agent-install.sh` 說明與摘要、README `--rate` 與週期掃描描述同步。
- 測試：48 單元＋10 整合全綠；`cargo fmt --check`、`cargo clippy --all-targets` 綠。
- **場域端到端驗證**（10.1.0.3、v0.1.1 binary、200pps、10.1.0.0/16）：`checked=65534`、`seen=239`、`dropped=0`、推送 14 次入庫（13×5000＋773）；掃描含重試約 10.9 分鐘。修正前舊版：`seen≈162–165`、每輪 `dropped=60000`、`checked` 僅剩 5,534 筆尾巴。
