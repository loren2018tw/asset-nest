# 01: 掃描逐筆節流與送訊中收訊

**What to build:** 主動掃描從「批次爆量＋每批 2 秒回覆窗」改為逐筆節流、送訊期間即時收訊；`AGENT_SWEEP_RATE_PPS` 成為真正的每秒送訊上限；收尾回覆窗的閒置判斷改以「回覆」為準（背景 ARP 不延長）。預設速率 1000 → 200（config、install 腳本、README 同步）。

**Blocked by:** 無

**Status:** ready-for-agent

- [ ] `probe_targets_platform`：逐筆 `send_frame`＋每筆後非阻塞 drain（`MSG_DONTWAIT`）＋deadline 制節流（間隔 `1/rate` 秒）
- [ ] `drain_window`：全數已回應／距最後一筆回覆 500ms／2 秒硬上限即結束；批次概念退場（`collect_replies`、`batch_pacing_delay` 移除；`reply_window_done` 保留、語意更新）
- [ ] 收集條件不變（opcode 2、sender ∈ 本輪目標、每址第一筆；成員判定 `HashSet`）
- [ ] 預設速率 1000 → 200（`config.rs`、`deploy/agent-install.sh`、`README.md`）
- [ ] 單元測試：`frame_interval`、`accept_reply`（linux）、`reply_window_done` 語意更新

## Comments

（待補）
