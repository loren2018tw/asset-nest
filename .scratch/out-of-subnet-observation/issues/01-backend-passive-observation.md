# 01: 被動 ARP 監聽與網段外紀錄（後端）

**What to build:** 探索掃描時在被探測網段的介面上被動監聽 ARP（與主動探測並行），把 sender 位址在 CIDR 外者寫入觀測影子層（`ip_presence.out_of_subnet=1`、來源 `arp_passive`），並提供 `GET /api/v1/observations/unmanaged` 清單端點；窗長以 `OBSERVATION_PASSIVE_WINDOW_SECS` 設定（預設 60、0＝停用）。

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

- [ ] migration 0007：`ip_presence.out_of_subnet` 欄位＋部分索引；不動 0006 以前
- [ ] `Prober::passive_observe(subnet, window)`：raw（含 auto 的 raw）以 AF_PACKET 監聽 opcode 1／2 的 sender、排除無效位址與本機；unprivileged 回空；純函式解析＋單元測試
- [ ] `run_discovery` 增 `passive_window`：並行啟動被動監聽、服務層過濾 CIDR 外、寫入現況（旗標）與事件（`arp_passive`）；報告加 `passive_seen`；窗長 0＝停用
- [ ] `OBSERVATION_PASSIVE_WINDOW_SECS` 設定＋`AppState.passive_window_secs`（builder；手動探索帶入）
- [ ] `GET /api/v1/observations/unmanaged`：last_seen 新→舊，含探測網段、首見、known／asset
- [ ] `.env.example`／README 更新
- [ ] 整合測試（stub）：寫入與事件、CIDR 內丟棄、passive_seen、端點（已知／未登錄）、unprivileged 空；真機 `#[ignore]` 短窗唯讀
- [ ] `cargo test`／`cargo fmt --check` 全綠
