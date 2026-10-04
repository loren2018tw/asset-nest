# 17: 移除 IP 清單 Gateway 欄位與排序

**What to build:** IP 管理清單不再顯示 Gateway 欄位、也不提供以此排序；gateway 僅在網段設定的編輯對話框可見。

**Blocked by:** None (can start immediately)

**Status:** done

- [x] 前端移除 Gateway 欄位與其 cell template
- [x] 後端排序白名單移除 `gateway`（`sort=gateway` 回 400）
- [x] 回應不再包含 `is_gateway` 欄位（後端型別、列建構、前端型別與相關測試同步移除；清除因此不再使用的程式碼）
- [x] 票 14 的 Gateway 排序測試與單元測試相應調整；既有行為回歸全綠
- [x] 前端 typecheck／lint 全綠

## Comments

實作完成（commit 訊息：`17 移除 IP 清單 Gateway 欄位與排序：移除欄位、排序白名單與 is_gateway（後端＋前端）`）。

- 後端：
  - `src/api/ips.rs`：`ListQuery` 排序欄位白名單註解更新為 `address`／`status`／`location`／`assignment`；`sort=gateway` 由 `IpSortField::parse` 回 `None`，API 層沿既有格式回 400 `validation_error` 並附 `details.field: "sort"`。
  - `src/ips.rs`：`IpEntry` 移除 `is_gateway`；`IpSortField` 移除 `Gateway` 變體與 parse 分支；`SortKey` 與比較器移除 gateway 鍵；`entry_v4`／`entry_v6`／`push_sort_key_v4`／`V4Scanner` 的 gateway 參數與欄位一併移除；不再使用的 `parse_gateway_v4`／`parse_gateway_v6` 刪除（網段 gateway 的 CRUD 與驗證不受影響）。
- 前端：
  - `api/ips.ts`：`IpEntry` 移除 `is_gateway`；`IpSortField` 移除 `"gateway"`。
  - `IpListPage.vue`：移除 Gateway 欄與 `body-cell-gateway` cell template；`toSortField` 移除 `"gateway"` 分支（未知欄位仍退回 `address`）。
- 測試：
  - `backend/tests/ips.rs`：`pool_and_gateway_flags_are_reported` 更名 `pool_flags_are_reported` 並移除 `is_gateway` 斷言；`list_sorts_by_whitelisted_columns_with_direction` 移除 gateway asc／desc 案例；`invalid_sort_and_dir_return_400` 新增 `sort=gateway` 400（`details.field: "sort"`、訊息含 `gateway`），有效值檢查改用 `sort=location`；超界頁測試改用 `sort=status`。
  - `backend/tests/ip_registry_v6.rs`：移除 `is_gateway` 斷言；gateway 位址登錄列改驗 `status: static`。
  - `backend/src/ips.rs`：單元測試 `pool_and_gateway_are_marked` 更名 `pool_is_marked`；刪除 `sort_by_gateway_groups_flagged_last_asc_first_desc`；`sort_parse_accepts_whitelisted_values_only` 改驗 `gateway` 回 `None`。
  - 衝突行為（IpOutOfSubnet／IpInPool）、精確位址搜尋、狀態篩選、其餘排序欄與分頁斷言皆維持不變、全綠。
  - 驗收：`pnpm test`（70 單元＋95 整合，共 165）、`cargo fmt --check`、`pnpm --filter frontend typecheck`、`pnpm lint:check` 全綠。
- 取捨：
  - `is_gateway` 一併自 API 回應移除（票面要求）；既有前端未在其他頁面使用此欄位，無相容負擔。
  - 網段 gateway 欄位本體（`Subnet.gateway`、網段 CRUD／驗證、編輯對話框）完全保留；`sort=gateway` 因移除白名單而回 400，屬預期契約變更。
- 實作 commit：`5ae077b`（`17 移除 IP 清單 Gateway 欄位與排序：移除欄位、排序白名單與 is_gateway（後端＋前端）`）
