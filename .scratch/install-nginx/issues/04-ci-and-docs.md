# 04 — CI 與文件：install-test 走 nginx、README

Status: done
Blocked by: 02, login-auth 05（同檔 `install-test.yml`）

## 目標

實作 `spec §5、§6`：CI 改走 nginx、README 更新。

## 範圍

- `.github/workflows/install-test.yml`：
  - 「驗證服務與 Kea 保留 roundtrip」步驟：`curl -fsS http://127.0.0.1/api/health`（改走 80）；加 `systemctl is-active --quiet nginx`。
  - 登入步驟（login-auth 票 05 引入）之 URL 改 `http://127.0.0.1/api/v1/login`。
  - 代理安裝與重跑步驟：`--server-url http://127.0.0.1`（去 8080）。
  - `/api/v1/agents` 查詢以 cookie（`-b`）走 80。
- `README.md`：
  - 一鍵安裝服務表加一列「nginx：反向代理 `http://<主機>/` → asset-nest `127.0.0.1:8080`」。
  - 網址段落（`http://<主機>:8080`）全面改 `http://<主機>/`；觀測代理安裝範例去埠號。
  - 「安裝後注意」防火牆措辭改限制 80／僅區域網路；加「HTTPS 尚未設定，日後以 certbot（Let's Encrypt）於 nginx 啟用」。
  - 升級段落：說明站台檔保留、`--force-nginx-conf` 語意；移除段落：nginx 站台設定會移除、套件保留。
- `deploy/install.sh` 摘要與 README 措辭一致（交叉檢查）。

## 驗收

- workflow 與 README 檢視；與 `spec §5、§6` 逐條核對。
- 手動觸發 `install-test`（兩個 OS）通過（由維護者執行；失敗時看 journalctl）。

## 注意

- 本票開工前 login-auth 票 05 須已落地（同檔）；與票 02 的 install.sh 摘要互相對應。
- 不要 `git commit`；不要動 `.scratch/` 內其他票。

## Comments

- 2026-10-07 完成（agent）。
- 變更摘要（僅 `install-test.yml` 與 `README.md`；未動 `install.sh`／`uninstall.sh` 程式邏輯）：
  - `.github/workflows/install-test.yml`：
    - 「驗證服務與 Kea 保留 roundtrip」：加 `systemctl is-active --quiet nginx`；健康檢查改 `curl -fsS http://127.0.0.1/api/health`（經 nginx）。
    - 執行代理安裝與重跑代理安裝：`--server-url http://127.0.0.1`（去 `:8080`）。
    - 登入 URL 改 `http://127.0.0.1/api/v1/login`；`/api/v1/agents` 兩處查詢（輪詢與逾時輸出）改走 80，維持 `-b /tmp/an-cookies`。
  - `README.md`：
    - 一鍵安裝：開頭加 nginx 前門；服務表改三個服務並加 `nginx` 列（反向代理 `http://<主機>/` → asset-nest `127.0.0.1:8080`，站台檔 `/etc/nginx/sites-available/asset-nest.conf`）；`asset-nest` 列網址改 `http://<主機>/`。
    - 觀測代理安裝範例去埠號：`--server-url http://<後端主機>`。
    - 常用選項：`--bind` 預設改 `127.0.0.1:8080`（`--no-nginx` 時 `0.0.0.0:8080`）；新增 `--no-nginx`、`--force-nginx-conf` 兩列。
    - 安裝後注意：防火牆改「限制 80 來源（或僅允許區域網路）」；加「HTTPS 尚未設定；日後以 certbot（Let's Encrypt）於 nginx 設定」；移除說明補「一併移除 nginx 站台設定、保留 nginx 套件」。
    - 升級：補「nginx 站台檔保留、`--force-nginx-conf` 才會覆寫（避免蓋掉日後 certbot 修改）」。
    - CI 段落：改「nginx／asset-nest／Kea 三個服務、`/api/health`（經 nginx）、Kea roundtrip 與代理心跳上線（含登入查詢）」。
- 檢視結果：
  - `python3 -c 'yaml.safe_load(...)'` → PASS（actionlint 本機未安裝，略）。
  - 與 spec §6 逐條（grep 檢核 7/7 PASS）：nginx active、health 經 80、登入走 80、代理安裝與重跑去埠、`/agents` 兩處走 80、workflow 無 `8080` 殘留。
  - 與 spec §5 逐條（grep 檢核 9/9 PASS）：nginx 服務列、`http://<主機>/`、觀測代理範例去埠、防火牆 80／區域網路、certbot 說明、升級站台檔保留與 `--force-nginx-conf`、移除站台設定／套件保留、無 `http://<主機>:8080` 殘留。
  - `install.sh` `print_summary` vs README 交叉檢查（PASS）：網址 `http://<主機>/（nginx → 127.0.0.1:8080）`、代理指令 `--server-url http://<主機>`、防火牆「限制 80 來源（或僅允許區域網路）」、HTTPS「日後以 certbot（Let's Encrypt）於 nginx 設定」語意一致（HTTPS 句近乎逐字相同）。
- 待驗項目：
  - 手動觸發 `install-test`（ubuntu-24.04／ubuntu-26.04）通過——需維護者於 GitHub 執行；失敗時看各步驟輸出與 `journalctl`（workflow 失敗步驟會印 asset-nest／asset-nest-agent／Kea 日誌）。
- 偏離說明：
  - README 另同步兩處範圍外小幅修正，避免文件與實作矛盾：常用選項表（`--bind` 預設因票 02 已改，原文錯誤；補兩項 nginx 選項）與 CI 段落（服務數與 health 描述）。
  - README HTTPS 句用「於 nginx 設定」（與 `print_summary` 近乎逐字一致），未採票內「啟用」用字（同義）。
  - workflow 失敗診斷步驟未加 nginx 日誌（非本票範圍；nginx 失敗時 `verify_nginx` 會 die 並提示 `systemctl status nginx`／`nginx -t`）。
  - `ss -ltn` 8080 僅 loopback 等檢查屬票 05 整合驗證範圍，未加入 CI。
