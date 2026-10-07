# 05 — 安裝與文件：AUTH_*、摘要、.env.example、README、CI 登入修正

Status: done
Blocked by: 02

## 目標

實作 `spec §6`。本票與 install-nginx 票 02 同動 `deploy/install.sh`、與 install-nginx 票 04 同動 `install-test.yml`；本票先落地。

## 範圍

- `deploy/install.sh`：
  - 新增 `configure_auth_credentials()`（比照 `configure_agent_auth_code`）：讀 `ENV_FILE` 既有 `AUTH_USERNAME`／`AUTH_PASSWORD`；重跑一律沿用；缺漏時 `AUTH_USERNAME="admin"`、`AUTH_PASSWORD="$(generate_password)"`；info 只說「已產生登入密碼（AUTH_PASSWORD）」**不印值**。
  - `main()`：`configure_auth_credentials` 於 `write_env_file` 之前。
  - `write_env_file()`：寫入 `AUTH_USERNAME=`／`AUTH_PASSWORD=` 兩行＋註解（指向 ADR-0021／`.env.example`）。
  - `print_summary()`：加「登入帳密：grep '^AUTH_' ${ENV_FILE}」；移除「（本系統尚無登入驗證）」字樣（防火牆／埠措辭留給 install-nginx 票 02）。
- `.env.example`：新增區塊——`AUTH_USERNAME`／`AUTH_PASSWORD`，說明：未設定或空白 → `admin`／`admin`；變更即所有裝置登出；工作階段 30 天；明文存放（檔案權限 0640、root）。
- `README.md`：
  - 一鍵安裝段補登入說明（預設帳密、變更方式＝編輯 env 後重啟、cookie 30 天、TLS 靠反向代理）。
  - 「安裝後注意」移除「本系統尚無登入驗證」句（不改埠相關措辭）。
- `.github/workflows/install-test.yml`：凡打受保護 `/api/v1/*`（如 `/api/v1/agents`）的步驟先登入：
  - `PW="$(sudo grep '^AUTH_PASSWORD=' /etc/asset-nest/asset-nest.env | cut -d= -f2-)"`
  - `curl -fsS -c /tmp/an-cookies -H 'Content-Type: application/json' -d "{\"username\":\"admin\",\"password\":\"$PW\"}" -X POST http://127.0.0.1:8080/api/v1/login >/dev/null`
  - 後續 `curl -b /tmp/an-cookies`；`/api/health` 維持免登入直接查。

## 驗收

- `bash -n deploy/install.sh`。
- 以假 `ENV_FILE` 人工驗證 `configure_auth_credentials`：首裝產生、重跑沿用（既有 `AUTH_*` 不被覆寫）。
- 文件交叉檢查：`.env.example`／README 與 `spec §3.1、§6` 一致。
- CI 變更待 install-nginx 票 04 合併後以手動觸發 install-test 驗證。

## 注意

- 密碼以既有 `generate_password` 產生；任何輸出不得含密碼值。
- 不要 `git commit`；不要動 `.scratch/` 內其他票。

## Comments

- 2026-10-07 完成（agent）。
- 變更摘要：
  - `deploy/install.sh`：新增 `configure_auth_credentials()`（重跑沿用既有 `AUTH_USERNAME`／`AUTH_PASSWORD`；缺漏時帳號 `admin`、密碼以 `generate_password` 產生；訊息只說「已產生登入密碼（AUTH_PASSWORD）」不印值）；`main()` 於 `configure_agent_auth_code` 後、`write_env_file` 前呼叫；`write_env_file()` 寫入兩行＋註解（指向 `docs/adr/0021`、`.env.example`）；`print_summary()` 加「登入帳密：grep '^AUTH_' ${ENV_FILE}」並移除「（本系統尚無登入驗證）」。
  - `.env.example`：新增「登入」區塊（未設定或空白逐欄位套預設 `admin`；變更即所有裝置登出；工作階段固定 30 天；明文存放、安裝產生之環境檔 root 所有權限 0640）。
  - `README.md`：一鍵安裝段補登入說明（首裝帳號 `admin`＋隨機密碼、未設定預設 `admin/admin`、改 env 後 `systemctl restart asset-nest`、cookie 30 天、TLS 靠反向代理 ADR-0022）；「安裝後注意」移除「本系統尚無登入驗證」（埠措辭不動）。
  - `.github/workflows/install-test.yml`：「驗證代理服務、環境檔權限與心跳上線」步驟先取 env 帳密 `POST /api/v1/login`（`-c /tmp/an-cookies`）再查 `/api/v1/agents`（`-b`，逾時 fallback 亦同）；`/api/health` 與其餘步驟不需調整（heartbeat 為免登入白名單；`pnpm test:kea` 直連 Kea 不經 API）。
- 驗收指令與結果：
  - `bash -n deploy/install.sh` → OK。
  - 假 `ENV_FILE` 以 `source deploy/install.sh` 測 `configure_auth_credentials`／`write_env_file`／`print_summary` → 17/17 PASS：首裝 `admin`＋32 字元隨機密碼；重跑既有值不覆寫；僅缺密碼時保留帳號；空白值視同缺漏；值含 `=` 完整沿用；環境檔含兩行與 ADR-0021 註解；摘要含 grep 提示、不含密碼值、已無「尚無登入驗證」。
  - 文件交叉檢查 `.env.example`／README 與 spec §3.1、§6 一致；`install-test.yml` YAML 解析 OK。
- 待驗項目：CI 為手動觸發，待 install-nginx 票 04 合併後以手動觸發 `install-test`（ubuntu-24.04／26.04）驗證登入＋代理查詢流程。
- 偏離說明：無（CI 登入帳號依票內範例寫死 `admin`，係因安裝腳本首裝固定寫入 `AUTH_USERNAME=admin`）。
