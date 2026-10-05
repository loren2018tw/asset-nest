# 02: 快速掃描與「最後可見」欄（核心）

**What to build:** 管理者在已開啟觀測的網段按「立即掃描」，系統對該網段的已指派位址做 ARP 探測，記錄每個位址的最後可見時間、最後 MAC 與最後檢查時間；IP 清單多一欄「最後可見」（N 天前／從未上線／未觀測，可排序，提示顯示來源與最後檢查時間）。位址首次被看到與換 MAC 會留下事件（供後續歷史查詢）。本票含 raw ARP 實作與零權限降級、探測模式環境設定，以及 systemd `CAP_NET_RAW` 最小權限。不含租約來源與排程。

**Blocked by:** 01 觀測開關與本機判定

**Status:** ready-for-agent

- [ ] 新資料表 `ip_presence` 與 `observation_event` 建立；`subnets` 既有觀測欄位沿用
- [ ] 快速掃描僅探測該網段已指派位址（測試以 stub 斷言目標集合）；未回應者更新最後檢查時間、不動最後可見
- [ ] 回應者寫入最後可見（來源 `arp`）；首次看到寫 `first_seen`；MAC 變更寫 `mac_changed`；同 MAC 重掃不重複寫事件
- [ ] `POST /subnets/{id}/sweeps`（mode=quick）同步回摘要；未開觀測／非同 L2／v6 回 400
- [ ] IP 清單回傳最後可見欄位與有效涵蓋狀態；`sort=last_seen`（NULL 固定最後）
- [ ] IP 清單前端欄位（三態＋來源提示）與「立即掃描」按鈕
- [ ] raw ARP 與零權限降級可由 `OBSERVATION_PROBE_MODE` 選擇（`auto` 遇權限問題降級）；systemd 單元含 `CAP_NET_RAW` 並更新 `.env.example`
- [ ] 測試：stub 探測邊界整合測試、ARP 解析單元測試、真機 `#[ignore]` 測試；`pnpm lint:check`／`pnpm typecheck` 全綠
