# 09: 連線主機 MAC 參考

**What to build:** 新增介面時顯示「目前連線主機 MAC」供參考（比照 Kealight 做法：伺服器由連線來源 IP 反查 ARP 表），並可一鍵填入 MAC 欄位。涵蓋後端 `GET /api/v1/peer-mac`、開發代理轉送來源資訊、前端新增介面表單提示。

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

- [ ] 後端 `GET /api/v1/peer-mac`：由連線來源 IPv4 反查 `/proc/net/arp`，回 `{mac}`；查不到（含 IPv6、incomplete）回 `{mac: null}`
- [ ] 開發情境：Vite proxy 轉送 `X-Forwarded-For`；僅在直接連線來源為 loopback 時採用（避免信任遠端偽造標頭）；正式環境（Rust 直接服務）用連線來源 IP
- [ ] 前端新增介面表單（指派對話框、資產對話框、資產端指派對話框）顯示「目前連線主機 MAC：xx（可供手動填入）」與「填入」按鈕
- [ ] 查不到時顯示「無法取得連線主機 MAC（需與本系統同一層網路）」（不做本機網卡 fallback，比照 Kealight）
- [ ] 單元測試涵蓋 ARP 解析（命中／未命中／incomplete／標題列）與來源 IP 選擇（loopback＋XFF、非 loopback 忽略 XFF）
- [ ] 整合測試涵蓋端點回應形狀（查不到回 `{mac: null}`）
