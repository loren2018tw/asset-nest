# asset-nest — IT 資產整合管理系統

Rust（axum）後端 + Quasar（Vue 3 / Vite）前端的整合系統。資產、IP 位址與 Kea DHCP 同步（保留、位址池與 gateway）功能陸續開發中；側邊欄「Kea」區段提供兩個唯讀頁面：**租約清單**（Kea DHCPv4 動態配發結果）與**系統狀態**（本系統服務狀態、Kea 版本、監聽介面、運行資訊、DHCPv4 摘要與連線診斷；首頁導向此頁）。

- 領域詞彙：`GLOSSARY.md`
- 決策記錄：`docs/adr/`
- Issue 追蹤：`.scratch/`（見 `AGENTS.md`）

## IP 觀測

每個 IPv4 網段可個別開啟「觀測」（v6 不可開）。開啟後系統在背景每 15 分鐘自動快速掃描一次：對該網段已指派與 Kea 有效租約的位址發 ARP 探測，記錄每個位址的「最後可見」時間、MAC 與來源（`arp`／`kea_lease`）；首次出現與 MAC 變更會留成事件（亦可在畫面上按「立即掃描」手動觸發）。

- v1 僅支援**本機同 L2** 的網段：本機沒有介面位址落在該網段時無法觀測（網段畫面會顯示提示）。
- ARP 探測預設 `auto`：先 raw（Linux `AF_PACKET`；systemd 單元已帶 `CAP_NET_RAW`），遇權限問題自動降級為零權限模式（UDP 觸發 kernel ARP 解析後讀 `/proc/net/arp`），降級只記一次警告。
- 觀測事件預設保留 365 天，每日自動清理過期事件；只影響歷史深度，**現況與宣告資料（指派／保留）完全不受影響**。
- **探索掃描**（每個網段可另開）：對網段**全部 host 位址**限速探測（上限為每秒 `OBSERVATION_DISCOVERY_RATE_PPS` 個），找出未指派卻實際在線的未知設備與未登錄 MAC；網段設定「探索間隔（分鐘）」（`discovery_interval_minutes`，留空＝全站預設 `OBSERVATION_DISCOVERY_INTERVAL_SECS`），網段畫面顯示上次探索時間，亦可按「立即探索」手動觸發。
- 相關環境變數（完整說明見 `.env.example`）：

| 變數 | 預設 | 說明 |
|------|------|------|
| `OBSERVATION_PROBE_MODE` | `auto` | ARP 探測模式：`auto`／`raw`／`unprivileged` |
| `OBSERVATION_RETENTION_DAYS` | `365` | 事件保留天數（正整數） |
| `OBSERVATION_DISCOVERY_INTERVAL_SECS` | `86400` | 探索掃描全站預設間隔秒數（正整數；網段可覆寫） |
| `OBSERVATION_DISCOVERY_RATE_PPS` | `1000` | 探索掃描每秒最多探測位址數（正整數） |

## 環境需求

- Node.js >= 24、pnpm 11
- Rust（stable，見 `rust-toolchain.toml`）

## 開發

```sh
pnpm install
pnpm dev          # Quasar dev server (9000) + Rust 後端 (8080)
```

開啟 http://localhost:9000。前端以相對路徑 `/api` 呼叫後端，開發時由 Vite proxy 轉發（見 `docs/adr/0004`）。

## 建置與部署

```sh
pnpm build        # quasar build → cargo build --release
./backend/target/release/asset-nest   # 同源服務 API 與前端，http://localhost:8080
```

部署物：`backend/target/release/asset-nest` + `frontend/dist/spa/`（可用 `WEB_DIST_DIR` 覆寫路徑）。環境設定見 `.env.example`。

## 一鍵安裝（全新 Ubuntu）

在全新安裝的 Ubuntu 24.04／26.04（需 root、需網路）上，一行指令安裝並啟用 Kea DHCP 3.2 與本系統：

```sh
curl -fsSL https://raw.githubusercontent.com/loren2018tw/asset-nest/main/deploy/install.sh | sudo bash
```

腳本會安裝 `asset-nest` 與 `isc-kea-dhcp4-server` 兩個 systemd 服務：

| 服務 | 說明 |
|------|------|
| `asset-nest` | 本系統：`http://<主機>:8080`；資料庫 `/var/lib/asset-nest/asset-nest.db`；設定 `/etc/asset-nest/asset-nest.env` |
| `isc-kea-dhcp4-server` | Kea DHCPv4（ISC Cloudsmith 3.2）；控制通道 `127.0.0.1:8000`（Basic 認證，憑證 `/etc/kea/asset-nest-api.*`） |

亦可從本機 checkout 執行（CI、開發機）：

```sh
sudo ./deploy/install.sh --source-dir "$PWD"
```

常用選項（完整說明：`./deploy/install.sh --help`）：

| 選項 | 說明 |
|------|------|
| `--kea-interfaces eth0` | Kea 監聽介面，逗號分隔（預設空＝不監聽；`*` 表全部） |
| `--kea-subnet 10.0.0.0/24` | 寫入單一 Kea 網段（id 1；供測試或單網段環境） |
| `--bind 127.0.0.1:8080` | asset-nest 綁定位址（預設 `0.0.0.0:8080`） |
| `--no-kea` | 只安裝 asset-nest；搭配 `--kea-url`／`--kea-username`／`--kea-password` 指向既有 Kea |
| `--force-kea-config` | 備份後覆寫既有 Kea 設定並重新產生控制通道憑證（預設保留） |

安裝後注意：

- 產生的 Kea 設定於最上層 `Dhcp4.option-data` 預設 DNS `8.8.8.8`（`domain-name-servers`），未另行覆寫的網段皆適用。
- **Kea 預設不監聽任何介面、也不含網段**（避免誤發 DHCP）。請編輯 `/etc/kea/kea-dhcp4.conf` 設定 `interfaces-config` 與 `subnet4`（記下 `id`）後 `systemctl restart isc-kea-dhcp4-server`；在本系統建立網段並填入相同 `kea_subnet_id`，保留、位址池與 gateway 由「Kea 同步」對齊（見 `docs/adr/0011`、`docs/adr/0013`）。
- 本系統尚無登入驗證；請以防火牆限制 8080 來源，勿暴露公網。
- 移除：`sudo ./deploy/uninstall.sh`（`--purge` 連資料與設定；`--remove-kea` 連 Kea 移除）。

**升級（已安裝系統）**：重跑上方一行安裝指令即完成——安裝腳本會在 `/opt/asset-nest/src` 以 `git fetch`＋`checkout` 更新至最新 `main`（可用 `--ref` 指定分支／標籤），重新建置並更新 `/opt/asset-nest`，最後重啟服務；Kea 設定與控制通道憑證保留、資料庫不動（migrations 於服務啟動時自動套用）。以 `--source-dir` 安裝者（開發機／CI）：在該 checkout `git pull` 後重跑 `sudo ./deploy/install.sh --source-dir "$PWD"`。

CI：`.github/workflows/install-test.yml` 為**手動觸發**（workflow_dispatch），在 `ubuntu-24.04`／`ubuntu-26.04` 實際執行安裝，驗證兩個服務、`/api/health` 與 Kea 保留 roundtrip（`pnpm test:kea`）；Cargo 建置有快取。

## 常用指令

| 指令 | 說明 |
|------|------|
| `pnpm dev` | 同時啟動前後端 |
| `pnpm build` | 建置前端與後端（release） |
| `pnpm lint:check` | 前端 lint / format 檢查 |
| `pnpm test` | 後端測試（cargo test） |
| `pnpm test:kea` | Kea 真機連線測試（version-get；讀 `.env` 的 `KEA_API_URL`） |
