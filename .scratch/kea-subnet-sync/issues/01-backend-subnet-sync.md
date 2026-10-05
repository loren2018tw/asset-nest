# 01: 後端網段層同步（pool 與 gateway）

**What to build:** `kea::http::Client`：`config_get_dhcp4()` 的 `subnets` 改為 `subnet-id → KeaSubnet`（cidr／pools 正規化範圍／gateway＝`routers` 值／原始物件 raw），新增 `subnet4_update()`（`subnet4-update`，需 subnet_cmds hook）。`kea::sync`：計畫與套用加入 pool 新增／刪除（嚴格對齊）與 gateway 變更（`null`＝移除 `routers`）；套用時由 raw 整段回寫、只改 `pools` 與 `routers` 條目，成功才計數，失敗記 `settings_error`；`config-write` 仍整批一次。API 計畫／報告型別擴充。補單元、stub 整合與真機唯讀測試。詳見 `.scratch/kea-subnet-sync/spec.md` 與 `docs/adr/0013`。

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

- [ ] `config_get_dhcp4` 解析 pools（範圍＋CIDR 正規化）與 routers；`kea_subnets()` 行為不變
- [ ] `subnet4_update`；整段物件原樣回寫、其他欄位與 option-data 保留
- [ ] 計畫：`pool_add`／`pool_delete`（數值排序）與 `gateway`（current／desired）＋ totals
- [ ] 套用：同範圍 pool 條目保留屬性、多餘刪除、gateway 改／增／移除；序列執行、單一網段失敗續行
- [ ] 報告：`pool_added`／`pool_deleted`／`gateway_updated`／`settings_error`；`config-write` 語意不變
- [ ] 單元測試：pool 正規化與 diff、gateway 抽取
- [ ] stub 整合測試：計畫差異、套用整段保留與嚴格刪除、無差異不呼叫、失敗報告、與保留混合單次寫檔
- [ ] 真機唯讀測試（`#[ignore]`）：config-get 解析 pool／gateway、`list-commands` 檢查 `subnet4-update`；不送修改命令
- [ ] `cargo test`／`cargo fmt --check` 全綠；票檔 Comments＋commit（不 push）

## Comments
