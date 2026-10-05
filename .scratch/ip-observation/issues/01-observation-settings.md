# 01: 觀測開關與本機判定

**What to build:** 管理者可在每個 IPv4 網段的編輯表單開啟／關閉觀測並持久化；系統以注入的探測邊界判定該網段是否與本機同 L2，並在表單與網段列表反映狀態。v6 網段不提供觀測；非同 L2 的網段開啟時顯示「v1 無法觀測」提示。本票先建立探測邊界（本機判定），實際探測方法由票 02 擴充。

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

- [ ] 網段編輯支援觀測開關（PATCH `observed`）；對 v6 網段開啟回 400 結構錯誤
- [ ] 網段詳情回應含 `observed` 與 `local`；列表摘要含 `observed`；`local` 由注入的探測邊界判定，測試可用 stub 控制
- [ ] 探測邊界掛載於共用狀態，預設實作以 Linux `getifaddrs` 判定本機是否有介面位址落在該 v4 子網
- [ ] 網段表單：觀測開關、v6 隱藏、`local=false` 顯示提示；儲存後往返一致
- [ ] 整合測試（HTTP＋記憶體 SQLite＋stub）與 `pnpm lint:check`／`pnpm typecheck` 全綠
