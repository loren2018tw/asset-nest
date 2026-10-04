# 13: IP 清單「位置」欄與位置搜尋

**What to build:** 網段的 IP 管理清單新增獨立「位置」欄（顯示指派對象所屬資產的位置），並讓關鍵字搜尋涵蓋位置。

**Blocked by:** None (can start immediately)

**Status:** done

- [x] 新增「位置」欄，位於「狀態／用途」與「指派對象」之間；有指派顯示該資產位置，未指派顯示「—」
- [x] 「指派對象」不再重複顯示位置（顯示資產描述、介面名稱／MAC，保留 hostname）
- [x] 關鍵字比對納入位置（子字串、大小寫無關），與 IP／資產描述／介面名稱／MAC 並列；搜尋框 placeholder 更新
- [x] v4 與 v6 行為一致
- [x] 後端整合測試涵蓋位置欄位與位置關鍵字（含無結果）；前端 typecheck／lint 全綠

## Comments

實作完成（commit `c8d78d5`）。

- 後端：`src/ips.rs` 的 `matches_query` 於既有位址文字、資產描述、介面名稱、MAC 之外補上 `assignment.asset_location` 子字串比對（`to_lowercase`、大小寫無關）；v4 枚舉與 v6 登錄共用同一函式，兩者行為一致。`IpFilter.q` 與 `matches_query` 註解同步更新。回應形狀不變（`assignment.asset_location` 已存在於票 05 的 `IpAssignment`）。
- 前端：
  - `IpListPage.vue`：於「狀態／用途」與「指派對象」之間新增「位置」欄（欄位名 `location`、`field` 由 `assignment.asset_location` 取值；未指派顯示「—」；本票不設 `sortable`，供票 14 銜接排序）；「指派對象」欄移除描述後的「（位置）」、只留資產描述與介面名稱／MAC（＋hostname）；搜尋框 placeholder 改為「搜尋 IP／資產描述／位置／MAC／介面名稱」。
  - `api/ips.ts`：`q` 參數註解補位置。
- 測試：
  - `backend/tests/ips.rs` 新增 2 個整合測試（另補 `create_asset`／`create_interface`／`assign_ip`／`register_ip`／`search_ips`／`encode`／`row` 等 helpers）：
    - `rows_report_assignment_location`：v4 手動／保留列與 v6 登錄列的 `assignment.asset_location` 正確；未指派列的 `assignment` 為 null。
    - `search_matches_assignment_location`：位置子字串（中文）與不分大小寫（`server room`／`SERVER ROOM`）命中已指派列；與狀態篩選並用（`q=機房&status=static` 命中、`reservation` 排除）；同一關鍵字可比對位置或既有欄位（`q=機` 命中兩列）；無結果；v4／v6 一致。
  - `src/ips.rs` 單元測試：`search_matches_assignment_fields` 補位置案例（中文子字串、`SERVER ROOM` 大小寫無關）；v6 搜尋測試關鍵字清單補 `機房`／`SERVER ROOM`；測試 helper 新增 `listed_at`（可指定位置），`listed` 維持預設「機房 A」。
- 驗收：`pnpm test`（60 單元＋所有整合全綠，`tests/ips.rs` 8 個）、`cargo fmt --check`、`pnpm --filter frontend typecheck`、`pnpm lint:check` 全綠。
- 取捨：本票不含排序；未指派列位置顯示「—」（與「指派對象」欄一致）；後端不變更任何 API 欄位，僅擴大 `q` 的比對範圍。
- 實作 commit：`c8d78d5`（`13 IP 清單位置欄與位置搜尋：獨立位置欄、q 涵蓋位置（後端＋前端）`）
