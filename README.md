# asset-nest — IT 資產整合管理系統

Rust（axum）後端 + Quasar（Vue 3 / Vite）前端的整合系統骨架。資產、IP 位址與 Kea DHCP 保留同步功能陸續開發中（目前僅骨架）。

- 領域詞彙：`GLOSSARY.md`
- 決策記錄：`docs/adr/`
- Issue 追蹤：`.scratch/`（見 `AGENTS.md`）

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

- **Kea 預設不監聽任何介面、也不含網段**（避免誤發 DHCP）。請編輯 `/etc/kea/kea-dhcp4.conf` 設定 `interfaces-config` 與 `subnet4`（記下 `id`）後 `systemctl restart isc-kea-dhcp4-server`；在本系統建立網段並填入相同 `kea_subnet_id`，保留才會同步（見 `docs/adr/0011`）。
- 本系統尚無登入驗證；請以防火牆限制 8080 來源，勿暴露公網。
- 移除：`sudo ./deploy/uninstall.sh`（`--purge` 連資料與設定；`--remove-kea` 連 Kea 移除）。

CI：`.github/workflows/install-test.yml` 為**手動觸發**（workflow_dispatch），在 `ubuntu-24.04`／`ubuntu-26.04` 實際執行安裝，驗證兩個服務、`/api/health` 與 Kea 保留 roundtrip（`pnpm test:kea`）；Cargo 建置有快取。

## 常用指令

| 指令 | 說明 |
|------|------|
| `pnpm dev` | 同時啟動前後端 |
| `pnpm build` | 建置前端與後端（release） |
| `pnpm lint:check` | 前端 lint / format 檢查 |
| `pnpm test` | 後端測試（cargo test） |
| `pnpm test:kea` | Kea 真機連線測試（version-get；讀 `.env` 的 `KEA_API_URL`） |
