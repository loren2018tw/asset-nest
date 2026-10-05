# 08: 資產脈絡與回收防呆

**What to build:** 資產清單多一欄「最後可見」（可排序）；資產詳情顯示最後可見、介面 MAC 可點開觀測歷史；刪除資產與取消指派／刪除保留的確認框顯示最後可見與最後 MAC，避免回收其實還在線的位址。

**Blocked by:** 02 快速掃描與「最後可見」欄（核心）、06 觀測歷史對話框與匯出

**Status:** done

- [x] 資產最後可見＝其已指派位址或介面 MAC 命中的現況列取最大值；清單與詳情皆回傳
- [x] 資產清單支援 `sort=last_seen`（NULL 固定最後）
- [x] 資產詳情顯示最後可見；介面 MAC 可開啟歷史對話框（重用票 06 元件）
- [x] 刪除資產、取消指派／刪除保留的確認框顯示最後可見與最後 MAC（僅提示、不阻擋）
- [x] 測試與 `pnpm lint:check`／`pnpm typecheck` 全綠

## Comments

實作摘要（commit `c9ce0cd`）：

- 後端：
  - `assets.rs`：`LAST_SEEN_EXPR`（相關子查詢）取資產命中的 `ip_presence.last_seen_at` 最大值；命中＝任一介面的指派 `(subnet_id, address)` 相同，或現況 `last_seen_mac` 與任一介面 MAC 不分大小寫相同（含未指派位址）。`last_seen_for_assets` 以當頁 id 單一查詢批次取值（無 N+1），`last_seen_for_asset` 重用同一查詢供詳情。
  - `SortField` 新增 `last_seen`（白名單外仍 400）；`push_order` 特判「最後可見」為 NULL 固定最後、不分升降冪（比照 IP 清單）。
  - `api/assets.rs`：清單列與詳情新增 `last_seen_at`（無命中為 `null`）；宣告資料完全不動（觀測唯讀，ADR-0014）。
- 前端：
  - `api/assets.ts`：新增 `AssetSortField`、`AssetListRow.last_seen_at`、`AssetDetail.last_seen_at`。
  - `AssetsPage.vue`：新增可排序「最後可見」欄（NULL 顯示「—」、其餘相對時間）；`toSortField` 白名單映射；刪除資產確認框加入最後可見與介面 MAC（HTML 轉義、明示僅提示不阻擋）。
  - `AssetFormDialog.vue`：編輯模式顯示最後可見；每個有效 MAC 的介面提供 history 按鈕，以 MAC 開啟 `ObservationHistoryDialog`（重用票 06 元件）。
  - `IpListPage.vue`／`AssignmentDialog.vue`：取消指派確認框（含 v6 登錄移除）附該位址「最後可見／最後 MAC」，沿用既有文案並註明僅提醒、不阻擋（`utils/observationHint.ts`）。
- 測試：`backend/tests/assets.rs` 新增 3 個整合測試（指派命中＋MAC 命中取最大與詳情欄位；大寫 MAC 命中、各資產獨立與 NULL；`sort=last_seen` 升降冪 NULL-last、分頁、匯出沿用與白名單外 400）。`cargo test` 296 passed／0 failed／6 ignored；`cargo fmt --check`、`pnpm lint:check`、`pnpm --filter frontend typecheck` 全綠。

整合複查備註：

- 資產端取消指派（`AssetFormDialog.vue` 的「取消指派」）未附觀測提示：該對話框的 `AssetAssignment` 沒有位址層級現況欄位，需另加 API 欄位才做得到；票 08 範圍明示 IP 清單取消即涵蓋。另「刪除保留」沒有獨立流程，由取消指派涵蓋。
- `sort=last_seen` 以 SQL 相關子查詢排序、值以 `last_seen_for_assets` 批次查詢，兩者為同一命中語意的等價實作（測試同時覆蓋值與序）。
