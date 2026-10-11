# 09: 觀測篩選改為「觀測 MAC 不符」

**What to build:** IP 清單觀測篩選移除「有未登錄 MAC」（`unknown_mac`），改新增「觀測 MAC 不符」（`observed_mac_mismatch`）：篩選結果逐列等於既有「觀測 MAC 不符」徽章（`ObservedMacMismatch`）——已指派位址、宣告介面 MAC 非空、且觀測 `last_seen_mac` 與其不同（不分大小寫；觀測 MAC 是否登錄不影響）。「非法佔用 IP」（`unassigned_seen`）不做任何改變；v6 維持不變（下拉隱藏、帶 `observed` 一律回空集合）。後端連根移除未知 MAC 能力（`IpObservedFilter::UnknownMac`、`ObservationView.known_macs`、`interfaces::macs`）。

**Blocked by:** None (can start immediately)

**Status:** done

- [x] `observed_mac_mismatch` 以合併衝突碼（`merged_conflicts` 的 `OBSERVED_MAC_MISMATCH`）比對，保證篩選與徽章一致
- [x] 舊值 `observed=unknown_mac` 回 400（白名單訊息更新為 `unassigned_seen 或 observed_mac_mismatch`）
- [x] 後端清理 `known_macs`／`interfaces::macs` 與相關單元／整合測試
- [x] 前端選項改為「觀測 MAC 不符」；型別與註解更新
- [x] 測試與 `cargo test`、`pnpm lint:check`／`pnpm typecheck` 全綠

## Comments

實作摘要：

- 後端：
  - `ips.rs`：`IpObservedFilter::UnknownMac` 換為 `ObservedMacMismatch`（查詢值 `observed_mac_mismatch`）；`matches_v4_filters` 以 `merged_conflicts` 合併碼（`conflicts::OBSERVED_MAC_MISMATCH`）比對，篩選結果與畫面徽章保證一致；`V4Context` 移除 `known_macs`；單元測試更新（parse 白名單、合併排序路徑、舊值回 `None`）。
  - `api/ips.rs`：白名單訊息改「觀測篩選須為 unassigned_seen 或 observed_mac_mismatch」；移除 `known_macs` 載入與 `HashSet` 匯入（`interfaces` 僅剩 `Warning`）。
  - `observation.rs`：`ObservationView` 移除 `known_macs` 欄位；`interfaces.rs` 移除 `macs()`；`conflicts.rs` 文件補述合併碼亦供篩選比對。
  - `tests/ips.rs`：`observed_filters_are_server_side_and_follow_badge_semantics` 重寫——涵蓋觀測到「別的已登錄 MAC」仍算不符（與舊值的關鍵差異）、同 MAC 大小寫不分、宣告介面無 MAC 不比對、未指派與池內位址排除、排序分頁／`status`／`q` 組合、v6 回空集合、`bogus` 與已移除的 `unknown_mac` 皆回 400。
- 前端：`api/ips.ts` 型別與註解、`IpListPage.vue` 選項標籤「觀測 MAC 不符」與註解。
- 文件：`spec.md` §Solution、需求 23 與 §讀取端篩選規則就地更新；`GLOSSARY.md` 不變（`ObservedMacMismatch` 詞義未動）、不開 ADR。
- 驗證：`cargo test`（lib 186 passed、`tests/ips` 25 passed、全庫 0 failed）、`cargo fmt --check`、`pnpm --filter frontend lint:check`、`pnpm --filter frontend typecheck`、`pnpm --filter frontend build` 全綠。
