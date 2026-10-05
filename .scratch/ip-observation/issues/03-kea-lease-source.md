# 03: Kea 租約來源

**What to build:** 快速掃描把 Kea 目前有效的 DHCP 租約納入：租約位址成為探測目標，`cltt` 記為「最後可見」（來源 `kea_lease`），因此不回應 ARP 的 DHCP 裝置（如 Windows）仍有活動訊號；IP 清單的來源提示可分辨 ARP 與租約。

**Blocked by:** 02 快速掃描與「最後可見」欄（核心）

**Status:** ready-for-agent

- [ ] 快速掃描目標集合＝已指派位址 ∪ 有效租約位址（`state=default`；以 `kea_subnet_id` 對應本機網段）
- [ ] 租約以 `cltt` 記為最後可見、來源 `kea_lease`；非 `default` 狀態不記
- [ ] 同一位址的 ARP 與租約訊號取最近者；Kea 未設定時行為不變
- [ ] 以 stub Kea 模式整合測試；`pnpm lint:check`／`pnpm typecheck` 全綠
