# 19: 清單排序修正與資產「已指派 IP」排序

**What to build:** 修正兩張清單的標頭排序（第二次點擊未反向、箭頭停在初始欄位），並讓資產清單「已指派 IP」欄可依第一筆已指派位址排序。

**Blocked by:** None (can start immediately)

**Status:** done

- [x] 資產清單：點擊可排序欄位後，第二次點擊反向，箭頭跟隨所點欄位（asc／desc 二態）
- [x] IP 清單：同上；點「位置／指派對象」等欄位時箭頭不再跳到 IP 欄
- [x] 資產清單「已指派 IP」可排序：依第一筆已指派位址（v4 先、v6 後、同族數值）；未指派固定最後；desc 為完全反向
- [x] 排序可與搜尋／篩選組合；清單分頁與資產匯出沿用同一排序
- [x] 後端整合測試涵蓋排序、方向、分頁、篩選組合與匯出；前端 typecheck／lint／build 全綠

## Comments

實作完成（commit `689d0a4`）。

- 前端（根因）：兩張 `q-table` 原本只傳 `:pagination`；Quasar 僅在同時有 `@update:pagination`（即 `v-model:pagination`）時，才會把父層更新併回表格內部狀態。因此第一次點擊送出新排序後，表格內部仍停在初始 `sortBy`／`descending`，第二次點擊重送同一方向，排序箭頭也永遠停在初始欄位。
  - `AssetsPage.vue`、`IpListPage.vue`：改為 `v-model:pagination="pagination"`，並加 `binary-state-sort`——同欄點擊只在 asc／desc 間切換，不會落入 Quasar 預設第三態「取消排序」（IP 清單取消時映射回 `address`，正是「箭頭跳到 IP 欄」的來源）。
  - `AssetsPage.vue`：「已指派 IP」欄 `sortable: true`。
- 後端：
  - `assets.rs`：`SortField` 新增 `AssignedIps`（`sort=assigned_ips`）；排序鍵為顯示序第一筆位址（v4 先、v6 後、同族依數值），未指派固定排最後（asc／desc 皆然），同鍵以 id 升冪決勝；desc 為位址鍵完全反向。SQLite 無 inet 型別，改以 `list_all_by_assigned_ips` 掃描後於 Rust 排序，再由 `take_page` 切頁；`list`／`list_all`（含匯出）共用此路徑。
  - `assignments.rs`：`address_sort_key` 改 `pub(crate)` 供資產排序重用。
- 測試：`backend/tests/assets.rs` 新增 `list_sorts_by_first_assigned_ip_with_unassigned_last`（數值序 vs 文字序、v4／v6、無指派最後、asc／desc、分頁、location 篩選組合、匯出沿用排序）。`cargo test`（93 單元＋所有整合）全綠；`cargo fmt --check`、`pnpm --filter frontend typecheck`、`pnpm lint:check`、`pnpm build:frontend` 全綠；另以真實 `asset-nest.db` 啟動後端 smoke `sort=assigned_ips` asc／desc 與無效值 400。
- 取捨：
  - `assigned_ips` 排序改為完整掃描後排序（不分頁查詢），與 IP 清單非預設排序同級成本。
  - 規格原「純顯示，不可排序」改為可排序（spec §2.1 追加定案 2026-10-05）；第三態「取消排序」不再使用。
- 實作 commit：`689d0a4`（`19 清單排序修正：反向切換與箭頭、資產「已指派 IP」排序（後端＋前端）`）

