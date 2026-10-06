# asset-nest — IT 資產整合管理系統

Rust（axum）後端 + Quasar（Vue 3 / Vite）前端的整合系統。資產、IP 位址與 Kea DHCP 同步（保留、位址池與 gateway）功能陸續開發中；側邊欄「Kea」區段提供兩個唯讀頁面：**租約清單**（Kea DHCPv4 動態配發結果）與**系統狀態**（本系統服務狀態、Kea 版本、監聽介面、運行資訊、DHCPv4 摘要與連線診斷；首頁導向此頁）。

- 領域詞彙：`GLOSSARY.md`
- 決策記錄：`docs/adr/`
- Issue 追蹤：`.scratch/`（見 `AGENTS.md`）

## IP 觀測

觀測執行由**觀測代理**負責：在每個要觀測的網段安裝一個代理，代理持續被動監聽 ARP、並週期主動掃描整個網段，將原始觀測推送回伺服器；伺服器端只保留 Kea 租約觀測、入庫與呈現（見 `docs/adr/0018`、`docs/adr/0019`）。安裝與行為詳見下方「觀測代理」。

- 網段沒有個別觀測開關：「已觀測」＝該網段有**在線代理**涵蓋（同網段多代理允許，取任一）；代理失聯時該網段顯示「未觀測」、既有資料保留不清除。「從未上線」＝有掃描紀錄但從未看到該位址，與「未觀測」不同。
- 沒有「立即掃描」；資料新鮮度以「最後回報時間」與「最後可見」呈現，掃描頻率與限速屬代理設定。
- **網段外觀測**：代理持續被動監聽 ARP（只收不送），sender 位址落在代理涵蓋網段 CIDR 外者記為「網段外觀測」（來源 `arp_passive`、旗標 `out_of_subnet`）；網段內者由主動掃描負責。可發現同 L2 但不在受管網段內的設備（錯設定、私接、其他網段延伸）；命中率取決於設備活動。
- **Kea 租約**（來源 `kea_lease`）仍是觀測來源之一：後端每 15 分鐘讀取目前有效租約（`state=default`、對應 `kea_subnet_id`），以 `cltt` 記為最後可見。
- 觀測事件預設保留 365 天、被拒回報預設保留 30 天，皆每日自動清理；只影響歷史深度，**現況與宣告資料（指派／保留）完全不受影響**。
- 相關環境變數（完整說明見 `.env.example`）：

| 變數 | 預設 | 說明 |
|------|------|------|
| `AGENT_AUTH_CODE` | 安裝時隨機產生 | 代理入庫認證碼；未設定＝入庫端點一律 503 |
| `AGENT_STALE_SECS` | `900` | 代理「在線」門檻秒數（正整數） |
| `AGENT_AUTH_FAILURE_RETENTION_DAYS` | `30` | 被拒回報保留天數（正整數） |
| `OBSERVATION_RETENTION_DAYS` | `365` | 事件保留天數（正整數） |

## 觀測代理

觀測執行外移給**觀測代理**：在每個要觀測的網段選一台主機安裝代理，代理持續被動監聽 ARP、並週期主動掃描整個網段，將原始觀測推送回伺服器；伺服器端只保留 Kea 租約觀測、入庫與呈現（見 `docs/adr/0018`、`docs/adr/0019`）。系統狀態頁（側邊欄「Kea → 系統狀態」）的「觀測代理」與「被拒回報」兩張表反映部署狀況。

### 安裝

在目標網段的一台 Ubuntu 24.04／26.04 主機上，一行指令安裝（需 root）。與後端同機時可直接讀後端環境檔：

```sh
curl -fsSL https://raw.githubusercontent.com/loren2018tw/asset-nest/main/deploy/agent-install.sh | \
  sudo bash -s -- --server-url http://<後端主機>:8080 \
  --auth-code-file /etc/asset-nest/asset-nest.env
```

`--auth-code-file` 可直接指向後端的環境檔（自動取 `AGENT_AUTH_CODE=` 行）。在另一台主機安裝時，先於後端執行 `sudo grep AGENT_AUTH_CODE /etc/asset-nest/asset-nest.env` 取得認證碼，再以 `--auth-code '<認證碼>'` 提供（或把該檔複製到代理主機後改用 `--auth-code-file`）。常用選項（完整說明：`./deploy/agent-install.sh --help`）：

| 選項 | 說明 |
|------|------|
| `--subnet <CIDR>` | 要觀測的 IPv4 網段；預設由本機介面自動偵測，多候選或找不到時必須明確指定 |
| `--name <name>` | 代理名稱（預設 hostname） |
| `--sweep-interval <secs>` | 主動掃描間隔秒數（預設 900） |
| `--rate <pps>` | 掃描每秒探測數上限（預設 1000） |
| `--version <tag>` | 自 GitHub Release 下載指定版本（預設 `latest`；例：`agent-v0.1.0`） |
| `--source-dir <path>` | 改以既有 checkout 自原始碼建置（不自動下載；開發機／CI） |

- 預設自 GitHub Release 下載對應架構的 **musl 靜態**產物（`asset-nest-agent-x86_64`／`asset-nest-agent-aarch64`）；未知架構或不存在的版本會明確拒絕。**下載失敗不會自動改為建置**，請依訊息排除，或改用 `--source-dir` 於本機建置（需要 Rust 與建置套件）。
- 重跑同一行指令即完成更新；既有 `AGENT_INSTANCE_ID` 與認證碼會保留（除非本次明確覆寫）。
- 移除：`sudo ./deploy/agent-uninstall.sh`（停用並移除服務與程式，保留 `/etc/asset-nest-agent`）；`--purge` 一併刪除設定。

### 行為與覆蓋語意

- **持續被動監聽**：只收不送地監聽 ARP；來源位址落在涵蓋網段**外**者記為「網段外觀測」（來源 `arp_passive`），網段內者由主動掃描負責。
- **週期掃描**：每 `AGENT_SWEEP_INTERVAL_SECS`（預設 900 秒）對網段全部 host 位址限速探測；回報 `checked`（全數送出）與 `seen`（有回應），因此「檢查過、沒回應」與「沒有資料」可以區分；安靜但活著的設備仍會被驗證存活。
- **心跳**：每 60 秒回報名稱、版本與涵蓋網段；後端 `AGENT_STALE_SECS`（預設 900 秒）內有回報視為「在線」。
- **覆蓋語意**：網段「已觀測」＝至少有一個在線代理涵蓋（同網段多代理允許，取任一）；代理失聯時該網段顯示「未觀測」、既有觀測資料保留不清除，代理恢復後自然接續。「從未上線」＝有掃描紀錄但從未看到該位址，與「未觀測」不同。

### 系統狀態頁的異常

- **未對應**：代理回報的網段 CIDR 與受管網段無精確對應，觀測不入庫。請修正代理的 `--subnet`／`AGENT_SUBNET_CIDR`，或先在系統建立該網段。
- **被拒回報**：認證碼不符的來源（來源 IP、自報名稱／版本、次數、最後嘗試）列於「被拒回報」表，不會寫入觀測；請修正該代理的認證碼或移除它。此表由後端每日清理（`AGENT_AUTH_FAILURE_RETENTION_DAYS`，預設 30 天）。

### 認證碼輪替與 TLS

- **輪替認證碼**：編輯後端 `/etc/asset-nest/asset-nest.env` 的 `AGENT_AUTH_CODE` → `sudo systemctl restart asset-nest`；再逐台更新代理（重跑安裝腳本帶新碼，或編輯 `/etc/asset-nest-agent/agent.env`）→ `sudo systemctl restart asset-nest-agent`。尚未更新的代理會被拒回報並顯示於系統狀態頁。
- **TLS**：後端本身不提供 TLS；跨越不可信網路時，請以反向代理（如 Caddy／nginx）提供 HTTPS，並讓代理以 `https://` 的 `--server-url` 連線（憑證須受代理主機信任）。

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
