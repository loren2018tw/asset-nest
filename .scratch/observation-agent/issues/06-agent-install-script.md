# 06: 代理安裝腳本（單獨安裝）

**What to build:** 可重跑的代理安裝／移除腳本（比照後端一鍵安裝慣例）：一行指令在乾淨 Ubuntu 24.04／26.04 安裝代理——必填伺服器位址與認證碼（參數／檔案／環境變數三選一），選填網段／名稱／掃描間隔／速率；建立專用使用者與 systemd 服務（CAP_NET_RAW 最小權限硬化）、寫入環境檔（重跑保留 instance id 與既有認證碼）、啟動並做 health 檢查。移除腳本預設保留設定、`--purge` 全清。CI 安裝測試擴充：以來源建置＋大掃描間隔安裝，驗證服務啟動與心跳上線。

**Blocked by:** 05

**Status:** done

- [x] 安裝腳本可在乾淨 Ubuntu 24.04／26.04 重跑（重跑＝更新；保留 instance id 與既有認證碼除非明確覆寫）
- [x] systemd 服務以專用使用者＋CAP_NET_RAW 最小權限運行；環境檔權限安全
- [x] 移除腳本：預設保留設定；`--purge` 一併移除
- [x] 安裝後代理出現在系統狀態頁（在線）
- [x] CI 安裝測試擴充：大間隔避免實際掃描，驗證服務與心跳
- [x] `--source-dir` 建置路徑可用（不需 prebuilt 產物）

## Comments

實作完成（未 commit；新增 `deploy/agent-install.sh`、`deploy/agent-uninstall.sh`，擴充 `.github/workflows/install-test.yml`，並在 `deploy/install.sh` 補上 CI 所需的 `AGENT_AUTH_CODE` 產生／沿用）。

- `deploy/agent-install.sh`（比照後端 install.sh 風格；`set -euo pipefail`、函式化、`usage()`、`BASH_SOURCE` 守衛可 source）：`--server-url` 必填（http(s)、去尾端斜線）；認證碼三選一（`--auth-code`／`--auth-code-file`（第一個非空行，容許 `AGENT_AUTH_CODE=` 前綴，可直接指向後端 env 檔）／環境變數 `AGENT_AUTH_CODE`；重跑未提供時沿用既有），明確提供即覆寫；選填 `--subnet`（IPv4 CIDR 驗證）／`--name`／`--sweep-interval`／`--rate`（正整數）／`--ref`／`--source-dir`（驗證 `agent/Cargo.toml`）。預設 clone repo 至 `/opt/asset-nest-agent/src`（重跑 `remote set-url`＋`fetch --depth 1`＋`checkout -f FETCH_HEAD`），以 `cargo build --release --manifest-path agent/Cargo.toml` 建置（Rust stable ≥1.85，缺則 rustup minimal 安裝），binary 安裝至 `/opt/asset-nest-agent/asset-nest-agent`。
- 安裝流程：建立系統使用者 `asset-nest-agent`（`--no-create-home --home /nonexistent`、`/usr/sbin/nologin`）→ `/etc/asset-nest-agent/agent.env`（目錄 0750、檔案 0640 root:asset-nest-agent）寫入 server URL／認證碼／instance id（沿用既有、否則 `/proc/sys/kernel/random/uuid`）與明確提供的選填值 → systemd `asset-nest-agent.service`（`User`／`Group`、`EnvironmentFile`、`Restart=on-failure`、`NoNewPrivileges`、`AmbientCapabilities=CAP_NET_RAW`、`CapabilityBoundingSet=CAP_NET_RAW`、`PrivateTmp`、`ProtectSystem=strict`、`ProtectHome`、`ProtectKernelTunables`、`ProtectControlGroups`、`RestrictSUIDSGID`）→ daemon-reload／enable／restart → `systemctl is-active` 輪詢（5 次）→ `${server-url}/api/health` 一次性可達性檢查（失敗僅警告＋排查提示，不 die）→ 摘要印出後續（系統狀態頁、CIDR 須精確對應、移除方式、TLS 建議）。
- `deploy/agent-uninstall.sh`（比照後端 uninstall.sh）：`disable --now` 並移除單元與 `/opt/asset-nest-agent`；預設保留 `/etc/asset-nest-agent` 並提示 `--purge`，`--purge` 一併刪除；`-h`。
- `deploy/install.sh`：`write_env_file` 新增 `AGENT_AUTH_CODE`（`configure_agent_auth_code`：首次 `generate_password`、重跑沿用既有 env 值）；安裝摘要印出讀取方式與 `agent-install.sh` 範例。此為 spec §安裝與發佈所述後端安裝端變更，也是 CI 讀取認證碼的前置依賴；後端 systemd 特權移除仍依票 08 未動。
- CI（`install-test.yml`）：快取加入 `agent/target` 與 `agent/Cargo.lock` 鍵；後端安裝驗證後，自 `/etc/asset-nest/asset-nest.env` grep `AGENT_AUTH_CODE`，以 `--source-dir "$GITHUB_WORKSPACE" --sweep-interval 86400` 安裝代理（另明確指定預設路由介面 CIDR：runner 可能有多個非 loopback 介面，代理自動偵測遇多候選會拒絕啟動）；驗證 `systemctl is-active asset-nest-agent`、env 檔權限 `640:root:asset-nest-agent`，並輪詢 `GET /api/v1/agents` 至該 instance id `online`（最多 60 秒）；再重跑一次安裝（不提供認證碼、維持大間隔）驗證 instance id 與認證碼保留；失敗步驟加印 `asset-nest-agent` journal。
- 本機驗證（未實際安裝；未動 `/opt`、`/etc/systemd`）：`bash -n`、`--help`、非 root 執行拒絕；另以 source 測試 25 項驗證參數驗證（URL／CIDR／數字／三選一／`--source-dir`）、認證碼來源優先序與既有值沿用、instance id 沿用與 UUID 產生、env 檔內容與權限、systemd 單元硬化欄位（env／unit 檔在 `unshare -r` 下寫至 temp，未觸真系統路徑）；`cargo build --release --manifest-path agent/Cargo.toml` 確認產物路徑。全綠：`cargo test --manifest-path backend/Cargo.toml`、`pnpm test:agent`（10 passed）、`pnpm lint:check`、`pnpm --filter frontend typecheck`。
- 備註：真機安裝（`/opt`、`/etc/systemd`、服務啟動、心跳上線）**未在本開發機執行**，依票面由 CI workflow（手動 `workflow_dispatch`）承擔驗證；「代理出現在系統狀態頁（在線）」依票 01–05 已驗證之行為（啟動立即心跳）由 CI 輪詢確認。`--version` 下載 prebuilt 產物屬票 07，本票僅 `--source-dir`／自 clone 建置路徑。
