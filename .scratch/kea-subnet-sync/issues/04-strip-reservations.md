# 04: subnet4-update 剝除 reservations（真機錯誤修正）

**What to build:** `kea::sync::apply_settings` 在 `subnet4-update` 前剝除 `config-get` 帶出的 `subnet4[].reservations`；stub 比照真機拒絕帶 `reservations` 的更新、並在取代後保留原保留（CfgHosts 分離）；整合測試覆蓋。真機實測：`10.1.0.2` 的 `config-get` 中 subnet 1／2 分別帶 892／99 筆 `reservations`，套用時 Kea 回 `result=1：must not specify host reservations with 'subnet4-update'.`。詳見 `.scratch/kea-subnet-sync/spec.md`。

**Blocked by:** None

**Status:** done

- [x] `apply_settings` 剝除 `reservations` 後才送 `subnet4-update`
- [x] stub：帶 `reservations` 的更新回 result 1（比照真機）；更新後保留原 reservations（比照 CfgHosts 分離）
- [x] 整合測試：fixture 含 reservations；套用成功且保留仍在
- [x] 真機唯讀確認：兩網段 reservations 筆數（未送修改命令）
- [x] 票檔 Comments＋commit（不 push）

## Comments

修正完成（commit `2f4d5df`，`04 Kea 網段層同步：subnet4-update 剝除 reservations（真機錯誤修正）`）。

- 根因：Kea 的 `config-get` 會把執行中的主機保留（host_cmds `operation-target: memory` 寫入 `CfgHosts`）合併呈現在 `subnet4[].reservations`；`subnet4-update` 原始碼明文拒絕帶 `reservations` 的物件（`must not specify host reservations with 'subnet4-update'.`）。真機唯讀實測 subnet 1 帶 892 筆、subnet 2 帶 99 筆。
- 修正：`apply_settings` 複製 `raw` 後先 `remove("reservations")` 再重建 `pools`／routers 並送 `subnet4-update`。主機保留存於 Kea `CfgHosts`，`cfg->replace(subnet)` 不觸碰（同原始碼 `subnet4-del` 才需顯式 `delAll4`），後續 `config-write` 照常把保留寫入設定檔。
- 測試：stub 新增比照真機的拒絕（payload 帶 `reservations` → result 1）與取代後保留原 `reservations` 的行為；`settings_fixture` 帶一筆保留；`apply_rebuilds_pools_and_gateway_keeping_other_fields` 加驗保留仍在，其餘既有測試（含失敗注入）不受影響。
- 真機未送任何修改命令；修正後由使用者重跑「Kea 同步」即可。
