# 一鍵安裝（Kea 3.2 + asset-nest）

對應決策：`docs/adr/0012`（一鍵安裝與同機佈署）；Kea 整合沿用 `docs/adr/0010`／`docs/adr/0011`。

## 目標

- 全新 Ubuntu 24.04／26.04 一行指令完成：Kea DHCP 3.2 + asset-nest + systemd 服務。
- 一鍵安裝說明寫在 `README.md`；CI 可手動實測整條安裝路徑。

## 範圍

- `deploy/install.sh`：單一自足安裝腳本，可重跑；`--source-dir` 供 CI／本機 checkout 使用。
- `deploy/uninstall.sh`：預設移除服務與程式、保留資料；`--purge`、`--remove-kea` 加碼清除。
- `.github/workflows/install-test.yml`：僅 `workflow_dispatch`（不隨 push），matrix `ubuntu-24.04`／`ubuntu-26.04`。
- 不改動同步邏輯；遠端 Kea 情境以 `--no-kea` + `--kea-url` 維持（ADR-0010 不變）。

## 決策摘要（詳見 ADR-0012）

- **Kea**：ISC Cloudsmith `kea-3-2` 套件庫；安裝 `isc-kea-dhcp4`、`isc-kea-hooks`；控制通道 `127.0.0.1:8000`（HTTP + Basic）。Kea 3.2 拒絕明文憑證，故以 `user-file`／`password-file` 檔（`/etc/kea/asset-nest-api.*`）設定。
- **Kea 全域 DNS 預設**：產生的 `/etc/kea/kea-dhcp4.conf` 於最上層 `Dhcp4.option-data` 預設 `domain-name-servers`＝`8.8.8.8`；未另行覆寫的網段皆適用（追加定案 2026-10-05）。
- **安全預設**：`interfaces` 與 `subnet4` 皆空（不主動服務）；`--kea-interfaces`／`--kea-subnet` 可先寫入。
- **asset-nest**：目標機原始碼建置（Node 24＋pnpm 依 `packageManager` 釘版＋rustup stable）；產物 `/opt/asset-nest`、環境檔 `/etc/asset-nest/asset-nest.env`、資料庫 `/var/lib/asset-nest`、服務 `asset-nest.service`（專用系統使用者）。
- **重跑語意**：更新程式並保留 Kea 設定與憑證；`--force-kea-config` 才覆寫 Kea 設定並重新產生憑證。

## 驗收

- `./deploy/install.sh --help` 可讀；非 root 執行給出明確錯誤。
- CI 兩版 Ubuntu：以 stdin 執行（`bash -s -- ... < deploy/install.sh`，模擬 `curl | bash`）完成安裝；`asset-nest` 與 `isc-kea-dhcp4-server` 皆 active；`/api/health` 200；`pnpm test:kea`（version-get + 保留 roundtrip）通過。
- README 含一鍵安裝章節。

## 實作記錄

- 主實作 commit：`b3c5a4e`（`deploy/install.sh`、`deploy/uninstall.sh`、CI workflow、README 一鍵安裝章節、ADR-0012；決策見 ADR-0012）。
- 追加（2026-10-05）：`render_kea_config` 於最上層新增 `option-data`（DNS `8.8.8.8`）；README 安裝後注意同步。
- 文件（2026-10-05）：README 一鍵安裝章節補「升級（已安裝系統）」——重跑安裝即更新（fetch／checkout、重建、重啟；Kea 設定與憑證、資料庫保留）；`--source-dir` 情境說明。
