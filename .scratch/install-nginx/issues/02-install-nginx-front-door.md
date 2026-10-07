# 02 — 安裝腳本：nginx 前門與 loopback 綁定

Status: done
Blocked by: login-auth 05（同檔 `deploy/install.sh`，先落地避免衝突）

## 目標

實作 `spec §2、§3、§5`：`install.sh` 加裝 nginx、改預設 bind、新增選項與摘要。

## 範圍

- 常數：`NGINX_SITE_AVAILABLE="/etc/nginx/sites-available/asset-nest.conf"`、`NGINX_SITE_ENABLED="/etc/nginx/sites-enabled/asset-nest.conf"`。
- 選項（`usage` 與 `parse_args` 同步）：
  - `--no-nginx`：跳過 nginx 安裝與設定。
  - `--force-nginx-conf`：覆寫既有站台檔（預設保留）。
- `BIND_ADDR` 預設邏輯：明示 `--bind` 優先；未明示時——`--no-nginx` → `0.0.0.0:8080`；否則 `127.0.0.1:8080`。（常數初值調整位置需在 parse 之後才能決定，注意既有 `case "$BIND_ADDR"` 驗證流程。）
- `install_nginx()`：`apt-get install -y nginx`；`systemctl enable nginx`。
- `check_port_80()`：80 被非 nginx 程序佔用 → `die`（訊息提示 `--no-nginx`，說明既有 web server 情境）。
- `configure_nginx()`：
  - 站台檔不存在或 `--force-nginx-conf` → 寫入 spec §3 template（含中文註解；`chmod 0644`）。
  - `ln -sf` 至 `sites-enabled`；`sites-enabled/default` 若存在（原廠）→ 移除連結。
  - `nginx -t` 通過後 `systemctl reload nginx`；失敗 `die` 並輸出錯誤。
- `main()`：`--no-nginx` 時跳過以上；收尾在 `wait_health` 後以 `curl -fsS http://127.0.0.1/api/health` 驗證經 nginx（失敗 `die`）。
- `print_summary()`：
  - 網址改 `http://<主機>/`（註明 `nginx → 127.0.0.1:8080`）；`--no-nginx` 時維持 `http://<主機>:8080`。
  - 加「HTTPS：尚未設定；日後以 certbot（Let's Encrypt）於 nginx 設定」。
  - 代理安裝指令去埠號（`--server-url http://<主機>`）。
  - 防火牆措辭：「以防火牆限制 80 來源／僅區域網路」。

## 驗收

- `bash -n deploy/install.sh`。
- 人工或 CI（與票 04 合併）：
  - 預設安裝：nginx active、`nginx -t` 通過、`http://127.0.0.1/api/health` 200、`ss -ltn` 8080 僅 `127.0.0.1`。
  - 重跑：自訂站台檔保留；`--force-nginx-conf` 覆寫。
  - `--no-nginx`：不裝 nginx、`0.0.0.0:8080` 對外、摘要正確。

## 注意

- 不覆寫已存在站台檔（除 `--force-nginx-conf`）；僅移除原廠 default 連結，不動其他站台。
- `wait_health` 沿用 loopback 檢查，不需改。
- 不要 `git commit`；不要動 `.scratch/` 內其他票。

## Comments

- 2026-10-07 完成（agent）。
- 變更摘要（`deploy/install.sh` 單檔）：
  - 常數：新增 `NGINX_SITE_AVAILABLE`／`NGINX_SITE_ENABLED`；開關 `WITH_NGINX=1`、`FORCE_NGINX_CONF=0`；`BIND_ADDR` 初值改空字串，留待 parse 後決定。
  - `usage()`：新增「nginx 前門選項」段（`--no-nginx`、`--force-nginx-conf`）；`--bind` 預設說明改「127.0.0.1:8080；--no-nginx 時 0.0.0.0:8080」；開頭說明與安裝後位置（站台檔）同步更新；檔頭安裝內容加 nginx（決策補 docs/adr/0022）。
  - `parse_args()`：新增兩選項；迴圈後才決定 BIND 預設（明示 `--bind` 一律優先；未明示時 `--no-nginx` → `0.0.0.0:8080`、否則 `127.0.0.1:8080`），之後才進 `validate_args`（既有 `case "$BIND_ADDR"` 驗證流程不變）。
  - `validate_args()`：`--no-nginx` 搭配 `--force-nginx-conf` → warn「無作用」並重置為 0（比照 `--force-kea-config`）。
  - 新增 nginx 區段：`install_nginx()`（`apt_install nginx`＋`systemctl enable --now nginx`）、`check_port_80()`（`ss -ltnpH 'sport = :80'`；非 nginx 佔用→印出 listeners 並 `die` 提示 `--no-nginx`；缺 `ss` 則 warn 跳過）、`render_nginx_site()`（spec §3 template 原文、quoted heredoc 使 `$host`／`$remote_addr` 不被展開）、`configure_nginx()`（不存在或 force→寫入＋`chmod 0644`；`ln -sf` 至 sites-enabled；僅當 `sites-enabled/default` 仍是原廠 symlink 時移除；`nginx -t` 失敗印錯誤並 `die`；通過後 `systemctl reload nginx`）。
  - `main()`：`install_base_packages` 後接 `check_port_80`／`install_nginx`／`configure_nginx` 區塊（`--no-nginx` 全跳過、fail-fast）；`wait_health` 後新增 `verify_nginx()`（`curl -fsS http://127.0.0.1/api/health`，--no-nginx 時 return 0）；開頭 info 加「（nginx 前門）」。
  - `print_summary()`：nginx 模式網址 `http://<主機>/（nginx → 127.0.0.1:8080）`、代理安裝指令去埠（`--server-url http://<主機>`）、加「HTTPS：尚未設定；日後以 certbot（Let's Encrypt）於 nginx 設定。」、防火牆措辭「以防火牆限制 80 來源（或僅允許區域網路）」；`--no-nginx` 維持 `http://<主機>:8080`／指令帶埠／防火牆 8080 措辭。
- 驗收指令與結果：
  - `bash -n deploy/install.sh` → OK。
  - `source deploy/install.sh` 純函式檢查（34/34 PASS）：BIND 預設四情境（預設→127.0.0.1:8080；--no-nginx→0.0.0.0:8080；明示 --bind 優先；--no-nginx＋明示 --bind 仍優先）、無冒號 --bind 仍 die、--no-nginx 時 force-nginx-conf warn＋重置、`render_nginx_site` 與 spec §3 逐字一致（8 項不變式＋確認未以 `$proxy_add_x_forwarded_for` 當指令）、摘要（nginx／no-nginx 網址、代理指令、HTTPS、防火牆措辭）、usage 含新選項、`check_port_80` 於目前環境（80 未被佔用）exit 0。
  - `check_port_80` 判斷式以假 `ss` 輸出模擬：非 nginx 佔用→die、nginx 佔用→續行、nginx＋他程序混合→die，三情境符合預期。
- 待驗項目（本機無法執行安裝，待票 05／CI）：
  - 預設安裝端到端：nginx active、`nginx -t` 通過、`http://127.0.0.1/api/health` 200、`ss -ltn` 8080 僅 127.0.0.1；`configure_nginx` 實際寫檔／`ln -sf`／移除原廠 default／reload 行為。
  - 重跑保留自訂站台檔；`--force-nginx-conf` 覆寫。
  - `--no-nginx`：不裝 nginx、維持 `0.0.0.0:8080`、摘要正確。
- 偏離說明：
  - `install_nginx()` 以 `systemctl enable --now nginx` 取代單獨 `systemctl enable`（apt 正常安裝本會啟動；`--now` 確保未自動啟動的環境也能於 `configure_nginx` reload）。已加註解。
  - `validate_args()` 另加 `--no-nginx`＋`--force-nginx-conf` 的 warn（比照 `--force-kea-config` 模式，票未明列但屬新選項互動）。
  - 未動 `wait_health`；未動 `.scratch/` 其他票、README、CI、uninstall.sh；未 `git commit`。
