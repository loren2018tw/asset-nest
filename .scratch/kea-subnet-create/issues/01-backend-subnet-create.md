# 01: 後端——完整同步建立缺少的受管網段

**What to build:** `kea::http::Client` 增 `subnet4_add()`（`subnet4-add`，`arguments.subnet4=[物件]`；subnet_cmds hook）。`kea::sync`：計畫新增 `subnet_add`（物件：pools／gateway）與 `totals.subnet_add`；缺少網段（id 不存在且無相同 CIDR）視 Kea 保留為空集合；相同 CIDR 掛其他 id → 預檢 error；套用順序改為「建立缺少網段 → 保留三相位 → 既有網段設定」，建立失敗記 `subnet_add_error` 並跳過該網段、成功記 `subnet_added`、單次 `config-write`。`subnets.rs`／`subnet_import.rs` 加 `kea_subnet_id` 範圍驗證（`0 < id < 4294967295`）。補單元與 stub 整合測試。詳見 `.scratch/kea-subnet-create/spec.md`、`docs/adr/0023`。

**Blocked by:** None (can start immediately)

**Status:** done

- [x] `subnet4_add`；建立物件組裝（pools `to_kea_string`、routers、空集合省略）
- [x] 計畫：`subnet_add`＋`totals.subnet_add`；同 CIDR 他 id 預檢 error；缺網段不呼叫 `reservation-get-all`
- [x] 套用：建立 → 保留 → 設定；失敗隔離；`subnet_added`／`subnet_add_error`；單次 `config-write`
- [x] `kea_subnet_id` 範圍驗證（API＋CSV 匯入）
- [x] 單元＋ stub 整合（建立、預檢、失敗、混合變更單次寫檔、既有行為不回歸）
- [x] `cargo test`／`cargo fmt --check` 全綠；票檔 Comments＋commit（不 push）

## Comments

實作完成（主實作 commit `d022a89`，`01 Kea 網段建立：後端 subnet4-add 與 sync 建立缺少網段（含 stub 測試）`）。

- `backend/src/kea/http.rs`：新增 `subnet4_add(&Value)`（`subnet4-add`、`arguments.subnet4 = [物件]`；`expect_success`），成功後由完整同步統一 `config-write`。
- `backend/src/kea/sync.rs`：
  - 計畫：`SyncPlanSubnet.subnet_add`（`{pools: [正規化 start-end、數值排序], gateway: string|null}`；`subnet_add` 為 `skip_serializing_if`、`gateway` 未設序列化 `null`）與 `totals.subnet_add`。
  - 配對：`kea_subnet_id` 不存在且無相同 CIDR（`same_network` 正規化，抽出 `same_cidr_owner` 預檢）→ 記為「將建立」，保留差異照算但**不呼叫** `reservation-get-all`（Kea 端視為空集合，existing 空、desired 由 DB assignments 推導、衝突標記語意不變）；該網段 `pool_add`／`pool_delete`／`gateway` 不重複填。相同 CIDR 掛其他 id → `error = "相同 CIDR 已由 Kea 網段 id {n} 使用"`、不計 `subnet_add`；`id` 相符但 CIDR 不符維持原整段錯誤（回歸測試）。
  - 建立物件組裝（`subnet_add_object`）：`id`／`subnet`／`pools`（`to_kea_string`、空則不帶）／routers option（重用 `set_gateway_option`，形狀同 update 路徑；未設 gateway 不帶）。
  - 套用：① 逐網段序列建立缺少者（成功含池與 gateway）→ ② 保留三相位 → ③ 既有網段網段層設定（`subnet_add.is_some()` 一律跳過）→ ④ 任一成功變更（含建立）才 `config-write` 一次；建立失敗記 `subnet_add_error`（含 Kea 原文）、該網段整段保留操作跳過且不記 failures、不阻擋其他網段；報告增 `subnet_added`／`subnet_add_error`，新建網段不另計 `pool_added`／`pool_deleted`／`gateway_updated`。
- `backend/src/subnets.rs`：常數 `KEA_SUBNET_ID_LIMIT = 4294967295`；`kea_subnet_id` 範圍驗證（`0 < id <` 上界，訊息「須介於 1 與 4294967294」）；v4 限定與全系統唯一不變。
- `backend/src/subnet_import.rs`：抽出 `parse_kea_subnet_id` 比照上界（錯誤碼沿用 `invalid_kea_subnet_id`；0／負數／非整數維持原「須為正整數」訊息、超上界為「超出範圍（須小於 4294967295）」）。
- 測試（+13：單元 +5、整合 +8）：
  - `kea::sync` +3 單元（建立物件組裝有／無 pools 與 gateway、同 CIDR 預檢正規化比對）；`subnets.rs` +1 單元（id 範圍上下界）。
  - `subnet_import.rs` +1 單元（解析上界）；`tests/subnet_import.rs` 匯入列擴充（4294967295 超界、4294967294 允許）；`tests/subnets.rs` +1 整合（API 範圍驗證）。
  - `tests/kea_sync.rs` +7 整合（缺網段計畫含 pools 數值排序／gateway／保留差異且不呼叫 `reservation-get-all`、空池與未設 gateway 的建立、同 CIDR 他 id 預檢、id 相符 CIDR 不符回歸、建立＋保留同回合與單次 `config-write`、建立失敗隔離＋無成功變更不寫檔、失敗時他網段續行）；stub 擴充 `subnet4-add`（比照真機：id 重複或前綴重複回錯、成功才入 subnets map，log 記錄呼叫）。
- 驗收：`cargo test --manifest-path backend/Cargo.toml` 全綠（175 單元＋231 整合、共 406 passed、5 ignored、0 failed）；`cargo fmt --check` 綠。
- 已知事項：
  - 真機段（建立 → 驗證 → 清理）依票 03；本票未動 `tests/kea_connectivity.rs`，未對真機送任何修改命令。
  - 前端型別與對話框（`subnet_add`／`totals.subnet_add`／`subnet_added`／`subnet_add_error`）依票 02。
  - 測試期間 `backend/target/debug/incremental`（8.2G、可重建）因磁碟一度寫滿而清除；未動 `asset-nest.db` 與 `tests/kea_connectivity.rs`。
