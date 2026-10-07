# spec — 一鍵安裝 nginx 反向代理（install-nginx）

- 狀態：已定案（經 2026-10-07 grilling 第二輪逐題確認，全部採納建議）
- 決策：`docs/adr/0022-nginx-reverse-proxy-front-door.md`（已接受）；`docs/adr/0019` 已加修訂註記
- 相關：登入與白名單見 `.scratch/login-auth/spec.md`（ADR-0021）
- 依賴：建議於 login-auth 票 01–05 落地後開工（同檔：`deploy/install.sh`、`install-test.yml`）

## 1. 背景與目標

一鍵安裝目前讓 asset-nest 直接綁 `0.0.0.0:8080`。目標：由 nginx 擔任唯一前門（`:80`）反向代理至 asset-nest；本次**只建 HTTP 設定檔**，HTTPS 日後以 certbot（Let's Encrypt）於 nginx 設定。asset-nest 改綁 loopback，之後 TLS 完成時不會殘留繞道的明文入口。

## 2. 入口架構

- 預設：`BIND_ADDR=127.0.0.1:8080`（`install.sh` 未明示 `--bind` 時）；nginx `:80` 全數反向代理。
- 明示 `--bind` 一律優先。
- 新選項 `--no-nginx`（比照 `--no-kea`）：跳過 nginx 安裝與設定；此時 `--bind` 預設維持 `0.0.0.0:8080`。
- 80 埠佔用：若 80 已被**非 nginx** 程序佔用 → 安裝前明確失敗並提示 `--no-nginx`。

## 3. nginx 安裝與設定

- `apt-get install -y nginx`；`systemctl enable nginx`。
- 站台檔 `/etc/nginx/sites-available/asset-nest.conf`＋`sites-enabled` 連結；移除原廠 `sites-enabled/default` 連結（僅當仍是原廠檔；不動其他既有站台）。
- 內容（template）：

```nginx
# asset-nest 反向代理（由 deploy/install.sh 產生；見 docs/adr/0022）
server {
    listen 80 default_server;
    listen [::]:80 default_server;
    server_name _;

    # HTTPS 尚未設定；日後以 certbot（Let's Encrypt）於本站台啟用。
    location / {
        proxy_pass http://127.0.0.1:8080;
        proxy_http_version 1.1;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        # 覆寫（非 $proxy_add_x_forwarded_for）：避免客戶端自帶值被視為第一段
        proxy_set_header X-Forwarded-For $remote_addr;
    }

    client_max_body_size 16m;  # CSV 匯入
}
```

- 生命週期：首次安裝建立；**重跑保留**既有站台檔，`--force-nginx-conf` 才覆寫（比照 `--force-kea-config`；避免覆蓋日後 certbot 的修改）。
- 套用後 `nginx -t` 必須通過才 `systemctl reload nginx`；失敗 → 明確錯誤。
- 安裝收尾驗證：`curl -fsS http://127.0.0.1/api/health`（經 nginx）通過才視為完成。

## 4. 來源 IP（XFF 信任模型；後端）

- nginx 之後後端 `ConnectInfo` 恆為 loopback；代理入庫（heartbeat／observations）現行「一律忽略 XFF、用連線來源」（ADR-0019）會讓 `source_ip` 全變 `127.0.0.1`——代理清單來源欄失真，且被拒回報以 `source_ip` 為 key，全部併成一列。
- 改為**沿用 `peer::resolve_peer_ip`**：連線來源為 loopback → 採 `X-Forwarded-For` 第一段；否則一律連線來源 IP。`backend/src/api/agents.rs` 兩端點（heartbeat、observations）改用它；peer-mac 不受影響（同一機制）。
- nginx 以 `X-Forwarded-For $remote_addr` **覆寫**（§3），後端信任第一段即為客戶端。
- ADR-0019 加修訂但書（直連情境仍忽略 XFF）——文件已改。

## 5. 移除與文件

- `deploy/uninstall.sh`：移除站台檔與 `sites-enabled` 連結、`systemctl reload nginx || true`；**保留 nginx 套件**（可能被他站台使用）；usage 補說明。
- `print_summary()`：`asset-nest：http://<主機>/（nginx → 127.0.0.1:8080）`；代理安裝 `--server-url http://<主機>`（去埠號）；加「HTTPS：日後以 certbot 設定」；防火牆措辭改「限制 80／僅區域網路」。
- `README.md`：一鍵安裝服務表加 nginx；網址、升級、移除、防火牆段落更新。

## 6. CI（`install-test.yml`）

- 健康檢查改 `curl -fsS http://127.0.0.1/api/health`（經 nginx）；加 `systemctl is-active --quiet nginx`。
- 代理安裝與重跑步驟 `--server-url http://127.0.0.1`（經 nginx 上報，同時實證 loopback 路徑）。
- 既有登入流程（login-auth 票 05）沿用，僅換埠號。

## 7. 非目標

- HTTPS／certbot／憑證自動續期（後續工作；本次只建 HTTP）。
- gzip、快取、其他 nginx 效能調校；管理主機上其他站台。
- PROXY protocol、非本機反向代理；`nginx` 套件的移除（uninstall 不刪套件）。

## 8. 測試矩陣

| 範圍 | 重點 |
|---|---|
| 後端單元 | `peer::resolve_peer_ip` 既有測試不變；確認公開可用 |
| 後端整合 | `agent_ingest`：loopback＋XFF → `source_ip` 為 XFF 值；loopback 無 XFF → `127.0.0.1`；非 loopback＋偽造 XFF → 連線來源（既有測試） |
| 安裝（CI＋人工） | nginx active；`nginx -t` 通過；`http://127.0.0.1/api/health` 200；`ss -ltn` 確認 8080 僅 loopback；重跑保留自訂站台檔；`--force-nginx-conf` 覆寫；`--no-nginx` 不裝 nginx 且維持 `0.0.0.0:8080`；uninstall 後站台檔消失、nginx 正常 |
| frontend | 不受影響（僅 `pnpm lint:check` 等既有檢查） |

## 9. 實作票

| # | 票 | 依賴 |
|---|----|------|
| 01 | 後端：代理入庫來源 IP 採 XFF（loopback 信任） | — |
| 02 | 安裝腳本：nginx 前門與 loopback 綁定 | login-auth 05（同檔 `deploy/install.sh`） |
| 03 | 移除腳本：站台設定移除 | 02 |
| 04 | CI 與文件：install-test 走 nginx、README | 02、login-auth 05（同檔 `install-test.yml`） |
| 05 | 整合驗證 | 01–04 |
