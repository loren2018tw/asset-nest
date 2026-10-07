# 05 — 整合驗證

Status: done
Blocked by: 01, 02, 03, 04

## 目標

對本 spec 全部票的實作做最終驗證，確認規格落地且無回歸。

## 步驟

1. 讀 `.scratch/install-nginx/spec.md`、`docs/adr/0022`、`docs/adr/0019` 修訂段，逐條核對。
2. 執行：
   - `cargo fmt --manifest-path backend/Cargo.toml -- --check`
   - `pnpm test`（重點 `--test agent_ingest --test agent_status`）
   - `bash -n deploy/install.sh deploy/uninstall.sh`
   - `pnpm --filter frontend typecheck`、`pnpm lint:check`（前端不受影響，確認仍綠）
3. `git status --short`、`git diff --stat`：不得動 `agent/`、前端功能碼；`deploy/` 僅本 spec 票範圍。
4. 安裝端到端（VM 或 CI）：
   - 預設安裝 → nginx active、`nginx -t`、`http://127.0.0.1/api/health` 200、8080 僅 loopback。
   - 經 nginx 登入／登出、代理心跳上線（`--server-url http://127.0.0.1`）。
   - 重跑保留站台檔；`--force-nginx-conf` 覆寫；`--no-nginx` 全跳過。
   - `uninstall.sh` → 站台檔移除、nginx 正常。
   - XFF：以 curl 模擬 loopback 帶 XFF 打代理端點（或整合測試已涵蓋）。
5. 規格落差與問題回報（票 01–04 範圍內可直接修，否則回報）。

## 產出

- 驗證報告（回覆）：每項指令與檢核結果、發現的問題與修正。
- 失敗項目：檔案、行號、輸出與建議。

## 注意

- 不要 `git commit`；不要主動改規格或 ADR（除錯字）。

## Comments

- 2026-10-07 驗證完成（agent）。結論：票 01–04 全數落地、無阻斷性落差；安裝端到端項目因本機無 root／apt／nginx，已以靜態交叉核對＋純函式測試代替，端到端待維護者以 CI 手動觸發（`install-test`）或 VM 執行，未假裝通過。
- 規格／ADR 逐條核對（PASS）：
  - spec §2：`BIND_ADDR` 預設 `127.0.0.1:8080`、`--no-nginx` → `0.0.0.0:8080`、明示 `--bind` 優先（實測 6 情境）；`check_port_80` 對非 nginx 佔用 `die` 並提示 `--no-nginx`。
  - spec §3：`render_nginx_site` 與 spec template **逐字一致**（diff 空）；含 XFF `$remote_addr` 覆寫（非 `$proxy_add_x_forwarded_for`）、`client_max_body_size 16m`、v4/v6 `default_server`、`server_name _`；站台檔建立／保留／`--force-nginx-conf` 覆寫；`sites-enabled` 連結；僅移除原廠 `default` 連結；`nginx -t` 通過才 reload；`verify_nginx` 經 80 驗 `/api/health`。`install_nginx` 以 `enable --now`（票 02 已記錄偏離，合理）。
  - spec §4：`agents.rs` heartbeat／observations 皆用 `peer::resolve_peer_ip`（loopback 取 XFF 第一段、否則連線來源）；ADR-0019 修訂但書在；ADR-0022 決策與實作一致。
  - spec §5：`uninstall.sh` 移除站台檔與連結、`reload || true`、保留 nginx 套件、usage 補說明；`print_summary` 網址／代理指令（去埠）／HTTPS／防火牆措辭符合；README 更新。
  - spec §6：workflow 加 `systemctl is-active --quiet nginx`、health 走 80、登入與 `/api/v1/agents` 走 80、代理安裝／重跑 `--server-url http://127.0.0.1`；`yaml.safe_load` 通過。
- 指令結果（全 PASS）：
  - `cargo fmt --manifest-path backend/Cargo.toml -- --check` → exit 0。
  - `pnpm test` → exit 0；`agent_ingest` 14 passed、`agent_status` 8 passed；三個 XFF 測試（`loopback_source_takes_first_xff_segment`、`loopback_without_xff_uses_connection_source`、`non_loopback_ignores_forged_forwarded_for`）明確驗證。
  - `bash -n deploy/install.sh deploy/uninstall.sh` → exit 0。
  - `pnpm --filter frontend typecheck` → exit 0；`pnpm lint:check`（oxfmt＋oxlint，65 檔）→ exit 0。
- git 範圍（PASS）：`agent/` 零變更（status／diff 空）；`deploy/` 僅 `install.sh`、`uninstall.sh`；本 feature 檔案僅 `backend/src/api/agents.rs`、`backend/tests/agent_ingest.rs`、`deploy/*.sh`、`.github/workflows/install-test.yml`、`README.md`、`.scratch/`、`docs/adr/0019`（＋設計期新檔 0022），與 diff stat 相符；未新增 commit；根目錄 `.env`／`asset-nest.db*`（gitignored）與 `.scratch/ime-composition/` 未動。
- 靜態／純函式交叉核對（可用 `source deploy/install.sh`）：template diff（A1）、XFF 覆寫（A2）、16m（A3）、default_server（A4）、server_name（A5）、非 proxy_add（A6）；BIND 決策 B1–B7；`NGINX_SITE_*` 常數 install／uninstall 一致（C1）；workflow 無 `:8080` 殘留、nginx 檢查在、登入／agents 走 80（D1–D6）；README 無 `http://<主機>:8080`、服務表含 nginx、防火牆／certbot／升級／移除段（E1–E7）。全數 PASS。
- 低風險觀察（非落差、未修改）：
  1. `check_port_80` 以行級 grep 判斷；若 nginx 與他程序共用同一 socket（SO_REUSEPORT）會視為「僅 nginx」而不 `die`——極端情境，非 spec 要求範圍。
  2. 明示 `--bind` 搭配 nginx 模式時，template 固定 `proxy_pass 127.0.0.1:8080`（spec §3 既定）；若使用者明示其他埠且不 `--no-nginx` 會不通。屬 spec 設計後果，建議維護者留意（未來若支援自訂埠需同步 template）。
- 待 CI／VM 驗證清單（未執行，明確保留）：
  1. 手動觸發 `install-test`（ubuntu-24.04／ubuntu-26.04）：nginx active、`nginx -t`、`http://127.0.0.1/api/health` 200、`ss -ltn` 8080 僅 loopback。
  2. 經 nginx 登入／登出、代理心跳上線（`--server-url http://127.0.0.1`）、`/api/v1/agents` cookie 查詢。
  3. 重跑保留自訂站台檔；`--force-nginx-conf` 覆寫；`--no-nginx` 不裝 nginx 且維持 `0.0.0.0:8080`。
  4. `uninstall.sh` 後站台檔／連結消失、`nginx -t` 通過、nginx 服務仍在。
  5. XFF 真機：loopback 帶 XFF curl 打代理端點（整合測試已涵蓋邏輯，真機待 CI／VM 補完）。
- 偏離／修正：無需修正；票 01–04 之偏離均已在各票 Comments 記錄（`enable --now`、`--force-nginx-conf` warn、README 範圍外小幅同步等），本票確認合理。
- 未 `git commit`。
