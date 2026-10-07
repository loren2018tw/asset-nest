# ADR-0022：nginx 反向代理前門與 loopback 綁定

- 狀態：已接受
- 日期：2026-10-07

## 背景

一鍵安裝目前讓 asset-nest 直接綁 `0.0.0.0:8080`。登入（ADR-0021）的 cookie 在 HTTP 下可被同網段竊聽；日後要給 Let's Encrypt（certbot）接手 TLS，需要一個穩定的 HTTP 前門。同時系統已有兩個依賴「連線來源 IP」的功能：peer-mac（僅 loopback 採信 XFF，已實作）與代理入庫來源紀錄（ADR-0019 規定一律用連線來源、忽略 XFF）——引入反向代理若不處理，代理來源會全部塌成 `127.0.0.1`。

候選：(A) nginx 前門＋asset-nest 改綁 loopback；(B) asset-nest 維持直接對外，nginx 只是額外入口；(C) 由 asset-nest 自帶 TLS。C 與 ADR-0019「傳輸安全靠部署」相悖、憑證自動化自建無益；B 在 TLS 完成後仍留明文繞道。

## 決策

- 一鍵安裝加裝 nginx，`http://<主機>:80` 反向代理至 `127.0.0.1:8080`；asset-nest `BIND_ADDR` 預設改為 loopback（`--no-nginx` 時維持直接對外）。本次只建 HTTP 站台；HTTPS 由日後 certbot 於 nginx 啟用。
- 站台檔由安裝腳本管理（`/etc/nginx/sites-available/asset-nest.conf`）：`listen 80 default_server`、`server_name _`、全站代理、`X-Real-IP` 與 **X-Forwarded-For 覆寫**為 `$remote_addr`（非 `$proxy_add_x_forwarded_for`）。重跑保留既有站台檔（`--force-nginx-conf` 才覆寫），讓日後 certbot 的就地修改不被安裝洗掉。
- **XFF 信任模型統一**：連線來源為 loopback（本機反向代理）→ 採信 XFF 第一段；否則一律連線來源 IP。代理入庫與 peer-mac 一致（ADR-0019 修訂）。
- `uninstall.sh` 只移除站台設定，保留 nginx 套件（可能被他站台使用）。

## 理由

- 單一前門：TLS 完成後不存在可行的明文繞道；certbot 接手時設定檔已在原位。
- 覆寫 XFF 而非追加：後端信任「第一段」，必須由 nginx 寫入真實連線位址，否則客戶端可自帶偽造值。
- loopback 限定的信任與 peer-mac 既有機制同構；非 loopback 直連（`--no-nginx`）仍不吃 XFF。
- 保留站台檔：`certbot --nginx` 會就地修改站台設定；安裝重跑不應回捲它。

## 後果

- 預設安裝下舊 `http://host:8080` 入口失效（改用 80）；`--no-nginx` 保留舊行為。
- 被拒回報與代理來源 IP 在 nginx 部署下恢復為真實客戶端 IP；直連時行為不變。
- `client_max_body_size 16m` 供 CSV 匯入；更大的匯入需調站台檔（`--force-nginx-conf` 會覆寫，自行調過者須注意）。
- 憑證與 HTTPS 是後續工作：`server_name _` 與 80 為 certbot HTTP-01 鋪路；屆時可能需調整站台檔。
