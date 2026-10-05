# 01: 觀測開關與本機判定

**What to build:** 管理者可在每個 IPv4 網段的編輯表單開啟／關閉觀測並持久化；系統以注入的探測邊界判定該網段是否與本機同 L2，並在表單與網段列表反映狀態。v6 網段不提供觀測；非同 L2 的網段開啟時顯示「v1 無法觀測」提示。本票先建立探測邊界（本機判定），實際探測方法由票 02 擴充。

**Blocked by:** None (can start immediately)

**Status:** done

- [x] 網段編輯支援觀測開關（PATCH `observed`）；對 v6 網段開啟回 400 結構錯誤
- [x] 網段詳情回應含 `observed` 與 `local`；列表摘要含 `observed`；`local` 由注入的探測邊界判定，測試可用 stub 控制
- [x] 探測邊界掛載於共用狀態，預設實作以 Linux `getifaddrs` 判定本機是否有介面位址落在該 v4 子網
- [x] 網段表單：觀測開關、v6 隱藏、`local=false` 顯示提示；儲存後往返一致
- [x] 整合測試（HTTP＋記憶體 SQLite＋stub）與 `pnpm lint:check`／`pnpm typecheck` 全綠

## Comments

實作完成（主實作 commit `2de96d5`，`01 IP 觀測：觀測開關、本機判定與 migration 0006`）。

- `backend/migrations/0006_observation.sql`：`subnets` 新增 `observed`／`discovery_enabled`／`discovery_interval_minutes`／`last_discovery_at`；新增 `ip_presence`（PK `(subnet_id, address)`、FK `ON DELETE CASCADE`）與 `observation_event`（append-only、`kind` 限 `first_seen`／`mac_changed`）及三個索引。migration 0001–0005 未動。
- `backend/src/probe.rs`：`Prober: Send + Sync`（`is_local(&self, subnet: &Subnet)`）；`SystemProber` 以 Linux `getifaddrs` 列舉介面 IPv4、判定是否有位址落在 v4 子網（v6／非法 CIDR／列舉失敗回 `false`、`freeifaddrs` 釋放）；單元測試 3 個（純函式包含判定、v6／非法 CIDR、loopback）。
- `backend/src/lib.rs`：`AppState.prober: Arc<dyn Prober + Send + Sync>`，`new` 預設 `SystemProber`、`with_prober` 供測試注入 stub。
- `backend/src/subnets.rs`：`Subnet` 與 `SubnetSummary` 新增 `observed`；摘要另加 `local`（`list` 收 `&dyn Prober` 判定）；`SubnetPatch.observed` 以合併後狀態驗證（v6 開啟回 400 `field=observed`）；`ValidSubnet.observed` 寫入 INSERT／UPDATE。
- `backend/src/api/subnets.rs`：詳情／建立／更新回應以 `SubnetDetail`（`serde(flatten)`＋`local`）回傳；清單以 `state.prober` 判定。
- 前端：`api/subnets.ts` 型別新增 `observed`／`local`；`SubnetFormDialog.vue` 新增「觀測」區塊（僅編輯既有 v4 網段顯示），開啟且 `local=false` 顯示「v1 無法觀測（本機非同 L2）」；`SubnetsPage.vue` 新增「觀測」欄（觀測中／未觀測，非同 L2 以 warning 色＋tooltip 區別）。
- 測試：`backend/tests/observation_settings.rs` 4 個（v4 往返、v6 阻擋且維持 0、已開啟 v4 改 v6 阻擋、摘要 stub `local` 差異）；`probe` 單元 3 個。
- 驗收：`cargo fmt --check`、`cargo test` 全綠（248 passed、0 failed、5 ignored）；`pnpm lint:check`、`pnpm --filter frontend typecheck` 全綠。
- 備註：新增網段時後端不支援 `observed`（票只要求 PATCH），表單因此僅在編輯既有 v4 網段顯示觀測區塊；`discovery_*` 欄位只進 migration，API 留待票 05。
