# ADR-0012：一鍵安裝——同機 Kea 3.2、原始碼建置、systemd 佈署

- 狀態：已接受
- 日期：2026-10-05

## 背景

需要在新裝的 Ubuntu 24.04／26.04 上以一行指令完成「Kea DHCP 3.2 伺服器＋本系統＋服務」。ADR-0010／0011 的 Kea 整合以遠端伺服器為前提，本 ADR 處理同機（單機）部署情境，並保留遠端情境。

實測（以 ISC Cloudsmith 3.2.1 deb 擷取二進位）：

- 套件 `isc-kea-dhcp4`（過渡套件 `isc-kea-dhcp4-server` 為空）、hook 在 `isc-kea-hooks`（`libdhcp_host_cmds.so`）；systemd 服務名 `isc-kea-dhcp4-server`，以 `_kea` 身分執行，設定檔 `/etc/kea/kea-dhcp4.conf`（`_kea:_kea 0640`，`config-write` 可寫回）。
- Kea 3.2 拒絕控制通道認證的明文 `user`／`password`，須以 `user-file`／`password-file` 檔案設定；401 回應與 ADR-0010 相符。
- `interfaces: []` 與空 `subnet4` 可通過驗證並啟動（控制通道照常運作）。

## 決策

- **入口為 `deploy/install.sh`**：支援 Ubuntu 24.04／26.04（`/etc/os-release` 檢查、其他版本拒絕）；root 執行；可重跑（重跑即更新）。
- **Kea 以 ISC Cloudsmith `kea-3-2` apt 套件庫安裝** `isc-kea-dhcp4`＋`isc-kea-hooks`；控制通道固定 loopback `127.0.0.1:8000`（`--kea-port` 可改），HTTP＋Basic 認證；憑證為 `/etc/kea/asset-nest-api.user` 與 `.password`（安裝時產生隨機密碼；`--force-kea-config` 重新產生），同一組寫入 asset-nest 環境檔。
- **安全預設**：`interfaces-config.interfaces` 與 `subnet4` 皆空——Kea 不主動服務；`--kea-interfaces`（可 `*`）與 `--kea-subnet CIDR` 供已決定網路策略者（含 CI）先寫入。網段仍由管理者維護，asset-nest 只同步保留（ADR-0011）。
- **asset-nest 於目標機以原始碼建置**：Node.js 24（NodeSource）、pnpm（依 `packageManager` 釘版）、rustup stable；不依賴預編譯產物或 release 流程。`--source-dir` 直接使用既有 checkout（CI／開發機）。
- **佈局**：產物 `/opt/asset-nest/`（`asset-nest`＋`web/`）；環境檔 `/etc/asset-nest/asset-nest.env`（0600 級權限、root:asset-nest）；資料庫 `/var/lib/asset-nest/asset-nest.db`；systemd 服務 `asset-nest.service`（專用系統使用者 `asset-nest`、`Restart=on-failure`、基本 hardening、`BIND_ADDR` 預設 `0.0.0.0:8080`）。
- **遠端 Kea 維持支援**：`--no-kea`＋`--kea-url/--kea-username/--kea-password`；ADR-0010／0011 不變。
- **CI 手動實測**：`.github/workflows/install-test.yml` 僅 `workflow_dispatch`，matrix `ubuntu-24.04`／`ubuntu-26.04`；實跑安裝後驗證服務、`/api/health` 與 Kea 保留 roundtrip；Cargo 建置以 `actions/cache` 快取（registry＋`backend/target`，key 綁 `Cargo.lock` 與 runner OS）。

## 理由

- 一鍵安裝需自足且可重現；原始碼建置與既有 repo 一致，無需先建立 release 流程。
- DHCP 誤服務的後果嚴重（發錯位址、蓋掉既有 DHCP），故預設不監聽、不含網段；要服務的人再明確開啟。
- Kea 3.2 的安全檢查以檔案制憑證為官方途徑；控制通道僅 loopback 可避免未授權遠端管理。
- 重跑保留 Kea 設定，讓安裝腳本可安全用於既有機器升級（Kea 設定在首次寫入後即為管理者資產）。

## 後果

- 安裝需網路與數分鐘建置時間（冷啟動含 apt／toolchain／release 建置）。
- Kea 設定首寫後不再由腳本覆寫；`--force-kea-config` 會備份後覆寫並更換憑證，兩側（Kea 憑證檔與 asset-nest 環境檔）由腳本一併更新。
- 本系統尚無登入驗證：`BIND_ADDR` 預設 `0.0.0.0:8080`，README 明示須以防火牆限制來源；未來認證實作前不得暴露公網。
- 同日另立兩支腳本：`deploy/uninstall.sh`（預設保留資料；`--purge`／`--remove-kea` 加碼）。
