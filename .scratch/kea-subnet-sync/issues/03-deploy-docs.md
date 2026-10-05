# 03: deploy 載入 subnet_cmds hook＋文件

**What to build:** `deploy/install.sh`：`configure_kea` 偵測 `libdhcp_subnet_cmds.so`（找不到即 die）、`render_kea_config` 的 `hooks-libraries` 加入第三條目、既有設定保留分支缺 hook 時 warn（說明網段層同步需此 hook）、安裝後提示文字更新；文件：`docs/adr/0013`、`GLOSSARY.md`（受管網段／完整同步）、`README.md`（功能與安裝說明）。詳見 `.scratch/kea-subnet-sync/spec.md`。

**Blocked by:** None (can start immediately；真機驗證待 hook 載入)

**Status:** done

- [x] `configure_kea` 偵測 subnet_cmds；`render_kea_config` 三條目；檔頭註解同步
- [x] 既有設定保留分支缺 `libdhcp_subnet_cmds` 時 warn（不改設定）
- [x] 本機驗證：`bash -n`；source 後以假 hook 路徑呼叫 `render_kea_config`，確認三條目皆在
- [x] 安裝後提示文字：受管網段的位址池與 gateway 由「Kea 同步」對齊
- [x] GLOSSARY／README 更新；ADR-0013 已立
- [x] 票檔 Comments＋commit（不 push；真機 hook 由使用者手動加入後另行重驗）

## Comments

實作完成（主實作 commit `6903f82`，`03 Kea 網段層同步：deploy 載入 subnet_cmds hook＋文件`）。

- `deploy/install.sh`：`configure_kea` 新增 `libdhcp_subnet_cmds.so` 的 glob 偵測（找不到即 `die`）；`render_kea_config` 的 `hooks-libraries` 加入第三條目；既有設定保留分支（`asset-nest-api.user`）缺 `libdhcp_subnet_cmds` 時 `warn`；檔頭註解與安裝後提示文字更新（「保留、位址池與 gateway 由『Kea 同步』對齊」）。
- 本機驗證：`bash -n` 通過；source 後設假 hook 路徑呼叫 `render_kea_config`，`hooks-libraries` 三條目（host_cmds／lease_cmds／subnet_cmds）皆在。
- 文件：`docs/adr/0013`（決策、後果與對 ADR-0011／0012 的擴充註記）；`GLOSSARY.md` 更新受管網段／完整同步並新增「網段層設定」詞條；`README.md` 功能與安裝後注意更新；`.scratch/kea-sync/spec.md` 加延伸指標。
- 真機（`10.1.0.2`）目前未載入 `libdhcp_subnet_cmds.so`（見票 01 Comments）。本環境無該機 SSH 權限，需由管理者於 `/etc/kea/kea-dhcp4.conf` 的 `hooks-libraries` 加入 `{ "library": "libdhcp_subnet_cmds.so" }`、`kea-dhcp4 -T` 驗證後 `systemctl restart isc-kea-dhcp4-server`；重跑安裝腳本亦會偵測既有設定並提醒。載入後跑 `pnpm test:kea` 的唯讀檢查即可確認 `subnet4-update` 可用。
