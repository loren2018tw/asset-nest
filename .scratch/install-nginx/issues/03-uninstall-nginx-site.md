# 03 — 移除腳本：站台設定移除

Status: done
Blocked by: 02

## 目標

實作 `spec §5`：`uninstall.sh` 移除 asset-nest 的 nginx 站台設定（套件保留）。

## 範圍

- `deploy/uninstall.sh`：
  - 新增常數 `NGINX_SITE_AVAILABLE`／`NGINX_SITE_ENABLED`（與票 02 相同路徑）。
  - 移除 `SERVICE_UNIT` 流程之後：`rm -f "$NGINX_SITE_ENABLED" "$NGINX_SITE_AVAILABLE"`；nginx 存在時 `systemctl reload nginx >/dev/null 2>&1 || true`。
  - `usage()` 補一行：「一併移除 asset-nest 的 nginx 站台設定（nginx 套件保留）」。

## 驗收

- `bash -n deploy/uninstall.sh`。
- 人工：`sudo ./deploy/uninstall.sh` 後 `/etc/nginx/sites-{available,enabled}/asset-nest.conf` 不存在、`nginx -t` 通過、nginx 服務仍在。

## 注意

- 不動 `/etc/nginx/sites-available/default`（原廠檔保留）；不解除安裝 nginx 套件；`--remove-kea` 行為不變。
- 不要 `git commit`；不要動 `.scratch/` 內其他票。

## Comments

- 2026-10-07 完成（agent）。變更 `deploy/uninstall.sh`：
  - 新增常數 `NGINX_SITE_AVAILABLE`／`NGINX_SITE_ENABLED`（與票 02 `install.sh` 逐字相同）。
  - `SERVICE_UNIT` 流程（`systemctl daemon-reload`）之後新增：`rm -f "$NGINX_SITE_ENABLED" "$NGINX_SITE_AVAILABLE"`；`command -v nginx` 成立時 `systemctl reload nginx >/dev/null 2>&1 || true`；加 `info` 訊息與註解（套件保留、失敗不影響移除）。
  - `usage()` 補「一併移除 asset-nest 的 nginx 站台設定（nginx 套件保留）」於（預設）說明下。
- 指令結果：
  - `bash -n deploy/uninstall.sh` → PASS。
  - 常數比對：`diff <(grep -h '^NGINX_SITE' deploy/install.sh) <(grep -h '^NGINX_SITE' deploy/uninstall.sh)` → 一致。
  - 隔離行為測試（`/tmp/opencode/test-uninstall-nginx-block.sh`，抽第 65–70 行、假 PATH／假 systemctl、mktemp 目錄）：6/6 PASS——nginx 存在時站台檔與連結移除＋reload 被呼叫；nginx 不存在時檔案仍移除且不呼叫 systemctl。
  - `usage()` 以行範圍抽函式執行，輸出含新增說明行（因腳本非純函式、source 會直接執行，故未整檔 source）。
- 待驗（票 05／人工）：`sudo ./deploy/uninstall.sh` 端到端——`/etc/nginx/sites-{available,enabled}/asset-nest.conf` 不存在、`nginx -t` 通過、nginx 服務仍在（需 root 與已安裝 nginx 環境）。
- 偏離：無（「nginx 存在」以 `command -v nginx` 判定；`|| true` 與重導向依票原文）。
