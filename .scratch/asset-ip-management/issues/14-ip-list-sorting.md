# 14: IP 清單標頭排序

**What to build:** IP 管理清單（v4 枚舉與 v6 登錄）的標頭可點擊排序，由伺服器端對符合篩選的全部位址排序後分頁。取捨已定案：v4 非 IP 欄排序採掃描後排序，成本與現行關鍵字搜尋同級，不設位址數上限。

**Blocked by:** None (can start immediately)

**Status:** done

- [x] 可排序欄位：IP（數值）、Gateway、狀態／用途、位置、指派對象；「衝突」「操作」不可排序
- [x] 預設不變：未帶排序參數時為 IP 數值升冪；點擊切換 asc／desc；切換排序時回第 1 頁
- [x] 狀態 asc 固定序「可用→池內→手動設定→保留」；指派對象依資產描述（不分大小寫）；同鍵以 IP 數值升冪決勝
- [x] 依位置或指派對象排序時，未指派（空白）固定排在最後（asc、desc 皆然）
- [x] 排序可與關鍵字／狀態篩選組合；v4（含出界指派列）與 v6 一致
- [x] 無效 `sort`／`dir` 回 400（比照 `/assets` 慣例）
- [x] 後端整合與單元測試涵蓋各欄、方向、空值、組合、無效參數；前端 typecheck／lint 全綠

## Comments

實作完成（commit `424a3c7`）。

- 後端：
  - `src/api/ips.rs`：`ListQuery` 新增 `sort`／`dir`；`sort` 白名單為 `address`／`gateway`／`status`／`location`／`assignment`、`dir` 為 `asc`／`desc`，無效值回 400 `validation_error` 並附 `details.field`（訊息比照 `/assets`：`無效的排序欄位：…`／`無效的排序方向：…（僅接受 asc／desc）`）；未提供時 `sort` 預設 `address`、`dir` 預設 `asc`。空字串視為未提供。
  - `src/ips.rs`：
    - `IpFilter` 加 `sort: IpSortField`／`dir: IpSortDir`（`#[default]` Address／Asc）；新增兩枚舉與 `parse`（未知值回 `None`）。
    - v4：預設排序（`address` 升冪，含顯式帶參）完整保留現況快速路徑——精確位址搜尋、無篩選時算術位移分頁、其餘情況 `V4Scanner` 串流掃描即時分頁（`consider` 的篩選邏輯抽為 `matches_v4_filters` 共用）。非預設排序（其餘四欄與 `address` 降冪）改為完整掃描符合 `q`／`status` 的位址（出界指派列以數值順序合併、一致納入）→建 `SortKey` →排序→取當頁；成本與關鍵字搜尋同級、不設位址數上限（已定案取捨，函式註記與本檔皆記明）。
    - v6：登錄列收集後一律以共用比較器排序（預設即數值升冪，行為與現況相同）；`static` 狀態、gateway 標記、篩選與分頁一致。
    - 比較語意集中在 `compare_sort_keys`：位址以 u128 統一（v4 為 u32 值）；`status` 固定序 `available→in_pool→static→reservation`（desc 反轉）；`location`／`assignment` 以 `assignment.asset_location`／`asset_description` 轉小寫比較（不分大小寫）；未指派（`None`）在兩欄 asc、desc 皆固定最後；`dir` 只反轉所選欄位，同鍵一律以位址數值升冪決勝（`then_with`）。
    - Gateway 方向語意（票未指定）：升冪採自然布林序——非 gateway 先、gateway 最後；降冪反之。
- 前端：
  - `api/ips.ts`：新增 `IpSortField` 型別（後端白名單五值）；`IpListParams` 加 `sort`／`dir`。
  - `IpListPage.vue`：IP、Gateway、狀態／用途、位置、指派對象五欄 `sortable: true`（衝突、操作維持不可排序）；Gateway 欄 `name` 由 `is_gateway` 改為 `gateway`（對應後端白名單，列資料 `field` 仍為 `is_gateway`，template slot 同步改名）；`pagination` 加 `sortBy`／`descending` 初始 `address` 升冪；`fetchIps` 送 `sort`／`dir`；`@request` 比照 `AssetsPage.vue`——以 `toSortField` 將 q-table 欄位名映射回白名單（未知值退回 `address`），排序欄位或方向變更時回第 1 頁；篩選變更（`reload`）維持目前排序、回第 1 頁。
- 測試：
  - `backend/tests/ips.rs` 新增 7 個整合測試：`list_sorts_by_whitelisted_columns_with_direction`（各欄 asc／desc、預設與顯式 `sort=address&dir=asc` 相同、狀態固定序、位置／指派對象同鍵位址升冪、未指派 asc／desc 皆最後）、`location_and_assignment_sorts_are_case_insensitive_with_blanks_last`（`server`／`機房 a`／`機房 B` 與 `alpha`／`beta`／`Bravo` 大小寫無關＋反轉＋空白最後）、`sorts_combine_with_q_and_status_filters`（q＋排序、status＋排序、q＋status＋排序、無結果）、`v6_list_sorts_consistently`（v6 預設／address desc／位置／指派對象／與 status 組合）、`non_default_sort_includes_out_of_subnet_assignments`（先 /24 指派 .200 再縮 /25；address desc 出界排第一、location 組內位址升冪、與 q 組合）、`invalid_sort_and_dir_return_400`（`sort=id`／`sort=conflicts`／`dir=sideways` 之 `details.field`；有效值不受影響）、`default_sort_stays_numeric_ascending_and_other_sorts_paginate`（預設不動、非預設排序＋分頁、超界頁空但總數不變）。
  - `src/ips.rs` 單元測試新增 9 個：`sort_parse_accepts_whitelisted_values_only`（含衝突／操作不可排序）、`sort_by_address_desc_lists_numerically_and_paginates`、`sort_by_gateway_groups_flagged_last_asc_first_desc`、`sort_by_status_uses_fixed_order_with_address_tie_break`、`sort_by_location_and_assignment_is_case_insensitive_with_blanks_last`、`sort_ties_break_by_address_ascending_in_both_directions`、`sort_combines_with_status_filter`、`non_default_sort_includes_out_of_subnet_assignments`、`v6_sorting_matches_v4_semantics`。
  - 既有測試 helper `filter`／`status_filter` 補 `..IpFilter::default()`；新增 `sort_filter`。
- 驗收：`pnpm test`（69 單元＋所有整合全綠，`tests/ips.rs` 15 個）；`cargo fmt --check`、`pnpm --filter frontend typecheck`、`pnpm lint:check` 全綠。
- 取捨：
  - 非預設排序（含 `address` desc）需完整掃描篩選後位址；不設位址數上限，與關鍵字搜尋同級成本——依 spec §7 定案，未另做反向算術位移等最佳化。
  - q-table 點擊週期沿用 Quasar 預設三態（asc→desc→取消排序）；取消時前端映射回 `address` 升冪送出，維持預設行為（比照 `AssetsPage.vue`）。
  - 同鍵決勝固定在位址升冪（即使 desc），確保 v4／v6 一致且可測。
- 實作 commit：`424a3c7`（`14 IP 清單標頭排序：伺服器端排序（IP／Gateway／狀態／位置／指派對象）（後端＋前端）`）
