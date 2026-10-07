# 01: 後端——完整同步建立缺少的受管網段

**What to build:** `kea::http::Client` 增 `subnet4_add()`（`subnet4-add`，`arguments.subnet4=[物件]`；subnet_cmds hook）。`kea::sync`：計畫新增 `subnet_add`（物件：pools／gateway）與 `totals.subnet_add`；缺少網段（id 不存在且無相同 CIDR）視 Kea 保留為空集合；相同 CIDR 掛其他 id → 預檢 error；套用順序改為「建立缺少網段 → 保留三相位 → 既有網段設定」，建立失敗記 `subnet_add_error` 並跳過該網段、成功記 `subnet_added`、單次 `config-write`。`subnets.rs`／`subnet_import.rs` 加 `kea_subnet_id` 範圍驗證（`0 < id < 4294967295`）。補單元與 stub 整合測試。詳見 `.scratch/kea-subnet-create/spec.md`、`docs/adr/0023`。

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

- [ ] `subnet4_add`；建立物件組裝（pools `to_kea_string`、routers、空集合省略）
- [ ] 計畫：`subnet_add`＋`totals.subnet_add`；同 CIDR 他 id 預檢 error；缺網段不呼叫 `reservation-get-all`
- [ ] 套用：建立 → 保留 → 設定；失敗隔離；`subnet_added`／`subnet_add_error`；單次 `config-write`
- [ ] `kea_subnet_id` 範圍驗證（API＋CSV 匯入）
- [ ] 單元＋ stub 整合（建立、預檢、失敗、混合變更單次寫檔、既有行為不回歸）
- [ ] `cargo test`／`cargo fmt --check` 全綠；票檔 Comments＋commit（不 push）
