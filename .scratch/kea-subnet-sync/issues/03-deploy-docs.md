# 03: deploy 載入 subnet_cmds hook＋文件

**What to build:** `deploy/install.sh`：`configure_kea` 偵測 `libdhcp_subnet_cmds.so`（找不到即 die）、`render_kea_config` 的 `hooks-libraries` 加入第三條目、既有設定保留分支缺 hook 時 warn（說明網段層同步需此 hook）、安裝後提示文字更新；文件：`docs/adr/0013`、`GLOSSARY.md`（受管網段／完整同步）、`README.md`（功能與安裝說明）。詳見 `.scratch/kea-subnet-sync/spec.md`。

**Blocked by:** None (can start immediately；真機驗證待 hook 載入)

**Status:** ready-for-agent

- [ ] `configure_kea` 偵測 subnet_cmds；`render_kea_config` 三條目；檔頭註解同步
- [ ] 既有設定保留分支缺 `libdhcp_subnet_cmds` 時 warn（不改設定）
- [ ] 本機驗證：`bash -n`；source 後以假 hook 路徑呼叫 `render_kea_config`，確認三條目皆在
- [ ] 安裝後提示文字：受管網段的位址池與 gateway 由「Kea 同步」對齊
- [ ] GLOSSARY／README 更新；ADR-0013 已立
- [ ] 票檔 Comments＋commit（不 push；真機 hook 由使用者手動加入後另行重驗）

## Comments
