# 02: 租約「到期時間」排序與預設排序

**What to build:** 租約清單「到期時間」欄可排序；預設以到期時間遞減（越晚到期越上面）。以自訂 `:sort-method` 實作：`null`／無法解析的到期時間固定排最後（升冪亦然，沿用全站「空白固定最後」慣例）、同值以 IP 數值升冪決勝；`ip_address` 數值排序維持可用。詳見 `.scratch/kea-lease-to-asset/spec.md`。

**Blocked by:** None

**Status:** done

- [x] `expires_at` 欄 `sortable: true`；`pagination` 預設 `sortBy: "expires_at"`、`descending: true`（每頁 50 不變）
- [x] 自訂 `:sort-method`：到期時間以時間戳比較（顯示維持本地時區）；`null`／無法解析固定排最後（升／降冪皆同）
- [x] 同到期時間以 IP 數值升冪決勝；`ip_address` 欄排序維持可用（`null`／無效值固定最後）
- [x] `pnpm --filter frontend typecheck`、`pnpm lint:check` 綠
- [x] 人工檢核：進頁即「越晚到期越上面」；點欄頭切換升降冪正確；無到期時間（—）恆在最後

## 注意

- 與票 01 同動 `frontend/src/pages/KeaLeasesPage.vue`；`pagination` 預設值以本票為準（`sortBy: "expires_at"`、`descending: true`）。
- 後端未動；不呼叫 Kea 寫入。

## Comments

實作完成（未 commit）：

- `KeaLeasesPage.vue`：`expires_at` 欄 `sortable: true`；`pagination` 預設 `sortBy: "expires_at"`、`descending: true`（越晚到期越上面；每頁 50 不變）。
- 新增 `:sort-method="sortLeases"`：到期時間以時間戳比較（顯示維持本地時區）；`null`／無法解析固定排最後（升／降冪皆同）；同值以 IP 數值升冪決勝。`ip_address` 改由同一函式以數值比較（欄位自身 `sort` 移除；`null`／無效值固定最後）。
- 型別配合 Quasar 契約：`sortLeases(rows: readonly KeaLease[], …): KeaLease[]`。
- 驗收：`pnpm --filter frontend typecheck` 綠；`pnpm lint:check` 綠（65 檔）；`pnpm build:frontend` 成功；`cargo test` 406 passed／0 failed／6 ignored。
- 人工檢核（進頁預設排序、欄頭切換、null 恆最後）待實機確認。
