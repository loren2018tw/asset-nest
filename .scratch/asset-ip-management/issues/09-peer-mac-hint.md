# 09: 連線主機 MAC 參考

**What to build:** 新增介面時顯示「目前連線主機 MAC」供參考（比照 Kealight 做法：伺服器由連線來源 IP 反查 ARP 表），並可一鍵填入 MAC 欄位。涵蓋後端 `GET /api/v1/peer-mac`、開發代理轉送來源資訊、前端新增介面表單提示。

**Blocked by:** None (can start immediately)

**Status:** done

- [x] 後端 `GET /api/v1/peer-mac`：由連線來源 IPv4 反查 `/proc/net/arp`，回 `{mac}`；查不到（含 IPv6、incomplete）回 `{mac: null}`
- [x] 開發情境：Vite proxy 轉送 `X-Forwarded-For`；僅在直接連線來源為 loopback 時採用（避免信任遠端偽造標頭）；正式環境（Rust 直接服務）用連線來源 IP
- [x] 前端新增介面表單（指派對話框、資產對話框、資產端指派對話框）顯示「目前連線主機 MAC：xx（可供手動填入）」與「填入」按鈕
- [x] 查不到時顯示「無法取得連線主機 MAC（需與本系統同一層網路）」（不做本機網卡 fallback，比照 Kealight）
- [x] 單元測試涵蓋 ARP 解析（命中／未命中／incomplete／標題列）與來源 IP 選擇（loopback＋XFF、非 loopback 忽略 XFF）
- [x] 整合測試涵蓋端點回應形狀（查不到回 `{mac: null}`）

## Comments

實作完成（commit 訊息：`09 連線主機 MAC 參考：ARP 反查端點與新增介面提示（後端＋前端）`）。

- 後端：
  - 新增 `backend/src/peer.rs`：`mac_from_arp_table`（忽略標題列、`00:00:00:00:00:00` incomplete 與無命中回 `None`）、`peer_mac`（IPv4 讀 `/proc/net/arp`；IPv6 或非 Linux 讀不到檔回 `None`，不做本機網卡 fallback）、`resolve_peer_ip`（來源 IP 選擇純函式，便於單元測試）。
  - 新增 `backend/src/api/peer.rs`：`GET /api/v1/peer-mac`，以 `axum::extract::ConnectInfo<SocketAddr>` 取得連線來源；`X-Forwarded-For` 僅在連線來源為 loopback（`127.0.0.1`／`::1`）時採用第一段，非 loopback 一律忽略（避免遠端偽造）；回應 `{ "mac": "aa:bb:cc:dd:ee:ff" }` 或 `{ "mac": null }`。
  - `backend/src/main.rs`：`serve` 改用 `into_make_service_with_connect_info::<SocketAddr>()`。
- 開發代理：`frontend/quasar.config.ts` 的 `/api` proxy 加 `xfwd: true`，讓 Vite 轉送 `X-Forwarded-For`。
- 前端：
  - 新增 `frontend/src/api/system.ts`（`fetchPeerMac()`）與共用元件 `frontend/src/components/PeerMacHint.vue`：掛載時查詢端點；有 MAC 顯示「目前連線主機 MAC：xx:xx:xx:xx:xx:xx（可供手動填入）」＋「填入」按鈕（emit `fill(mac)`）；查不到或查詢失敗顯示「無法取得連線主機 MAC（需與本系統同一層網路）」（灰色小字）。
  - `AssignmentDialog.vue`：掛在新增介面卡片內，`@fill` 填入 `newInterface.mac`。
  - `AssetFormDialog.vue`：掛在「網路介面」區塊標題列下方，`@fill` 填入最後一筆 MAC 空白的介面草稿；無空白草稿時 `$q.notify` 提示「請先新增介面」。
- 測試：
  - 單元測試 4 個（`backend/src/peer.rs`）：ARP 命中／未命中／incomplete、標題列與短行忽略、loopback＋XFF（含 `::1`）／loopback 無 XFF／非 loopback 忽略 XFF、XFF 多段取第一段與解析失敗退回。
  - 整合測試 2 個（`backend/tests/peer_mac.rs`）：以注入 `ConnectInfo` 模擬來源（TEST-NET `203.0.113.7` 與 IPv6 `2001:db8::7`），斷言 200 與 `{mac: null}` 形狀。
  - 驗收：`cargo test`（43 單元＋53 整合全綠）、`cargo fmt --check`、`pnpm typecheck`、`pnpm lint:check`、`pnpm build` 全綠；另以真實伺服器 smoke：loopback 直連→`null`、loopback＋XFF `140.128.179.51`→正確 MAC `88:d7:f6:56:1f:18`、非 loopback＋偽造 XFF→`null`。
- 與規格差異／取捨：
  - 不做本機網卡 fallback（比照 Kealight；跨網段、VPN、IPv6 一律 `{mac: null}`）。
  - 票列「資產端指派對話框」目前尚未存在（屬票 10 範圍），本次掛到既有兩個新增介面表單（指派對話框、資產對話框）；共用元件 `PeerMacHint.vue` 屆時可直接掛上。
  - 缺少 `ConnectInfo` 時由 axum 回預設 500；正式啟動一律帶 `into_make_service_with_connect_info`，故未特別處理（整合測試以注入情境為主）。
- 實作 commit：（commit 後補）
