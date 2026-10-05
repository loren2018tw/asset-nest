# 01: 後端網段層同步（pool 與 gateway）

**What to build:** `kea::http::Client`：`config_get_dhcp4()` 的 `subnets` 改為 `subnet-id → KeaSubnet`（cidr／pools 正規化範圍／gateway＝`routers` 值／原始物件 raw），新增 `subnet4_update()`（`subnet4-update`，需 subnet_cmds hook）。`kea::sync`：計畫與套用加入 pool 新增／刪除（嚴格對齊）與 gateway 變更（`null`＝移除 `routers`）；套用時由 raw 整段回寫、只改 `pools` 與 `routers` 條目，成功才計數，失敗記 `settings_error`；`config-write` 仍整批一次。API 計畫／報告型別擴充。補單元、stub 整合與真機唯讀測試。詳見 `.scratch/kea-subnet-sync/spec.md` 與 `docs/adr/0013`。

**Blocked by:** None (can start immediately)

**Status:** done

- [x] `config_get_dhcp4` 解析 pools（範圍＋CIDR 正規化）與 routers；`kea_subnets()` 行為不變
- [x] `subnet4_update`；整段物件原樣回寫、其他欄位與 option-data 保留
- [x] 計畫：`pool_add`／`pool_delete`（數值排序）與 `gateway`（current／desired）＋ totals
- [x] 套用：同範圍 pool 條目保留屬性、多餘刪除、gateway 改／增／移除；序列執行、單一網段失敗續行
- [x] 報告：`pool_added`／`pool_deleted`／`gateway_updated`／`settings_error`；`config-write` 語意不變
- [x] 單元測試：pool 正規化與 diff、gateway 抽取
- [x] stub 整合測試：計畫差異、套用整段保留與嚴格刪除、無差異不呼叫、失敗報告、與保留混合單次寫檔
- [x] 真機唯讀測試（`#[ignore]`）：config-get 解析 pool／gateway、`list-commands` 檢查 `subnet4-update`；不送修改命令
- [x] `cargo test`／`cargo fmt --check` 全綠；票檔 Comments＋commit（不 push）

## Comments

實作完成（主實作 commit `b05d616`，`01 Kea 網段層同步：後端 pool／gateway 差異與 subnet4-update（client＋sync＋stub／真機測試）`）。

- `backend/src/kea/http.rs`：
  - `Dhcp4Config.subnets` 由 `subnet-id → CIDR` 改為 `subnet-id → KeaSubnet`（`cidr`、`pools: Vec<Ipv4Range>`、`gateway`、`raw`）；`kea_subnets()` 改為映射 CIDR、行為不變（`kea_view` 與真機測試無退步）。
  - `Ipv4Range`（端點皆含；`to_compact_string`／`to_kea_string`）與解析：範圍 `a - b`／`a-b`、CIDR 展開為整段（v4 prefix pool 可配發 network／broadcast，依 Kea ARM）；頭尾顛倒或無效回 `None`。
  - routers 抽取：`name == "routers"` 或 `code == 3`＋`space == "dhcp4"`（未指定視為 dhcp4）；空字串視為未設。
  - 新增 `subnet4_update()`（`subnet4-update`，`arguments.subnet4 = [整段]`）與 `list_commands()`（唯讀診斷）。
- `backend/src/kea/sync.rs`：
  - `compute_plans` 改讀 `config_get_dhcp4()`；CIDR／subnet-id 檢查不變；新增期望 pool 解析、`pool_diff`（同範圍視為相同、不重建）與 gateway 差異。
  - 計畫：每網段 `pool_add`／`pool_delete`（正規化 `start-end`、數值排序）與 `gateway {current, desired}`；`totals` 增 `pool_add`／`pool_delete`／`gateway`。
  - 套用：保留三相位後逐網段序列 `subnet4-update`；由 `raw` 複製整段、`pools` 重建（同範圍既有條目原樣保留→`client-classes` 等屬性不遺失、缺的補 `{"pool":"a - b"}`、多的移除）、routers 改值／新增／移除；成功才計 `pool_added`／`pool_deleted`／`gateway_updated`，失敗記 `settings_error` 續行；任一成功變更（含網段層）才 `config-write` 一次。
- 測試：`kea/http.rs` +3 單元（pool 解析、subnet 解析、routers 抽取）；`kea/sync.rs` +2 單元（pool_diff、gateway option 加減改）；`tests/kea_sync.rs` +6 整合（計畫差異、套用整段保留／嚴格刪除、gateway 移除、無差異不推送、`subnet4-update` 失敗不阻擋保留、保留＋網段層單次寫檔）；`tests/kea_connectivity.rs` +1 `#[ignore]` 唯讀（config-get 解析 pool／gateway、`list-commands` 檢查 `subnet4-update`）。
- 驗收：`cargo test --manifest-path backend/Cargo.toml` 全綠（104 單元＋137 整合、共 241 passed、5 ignored、0 failed）；`cargo fmt --check` 綠。
- 真機：`10.1.0.2` 目前 `hooks-libraries` 未載入 `libdhcp_subnet_cmds.so`（`list-commands` 無 `subnet4-update`），本票未對真機送任何修改命令；載入 hook 後再跑真機測試（見票 03）。
