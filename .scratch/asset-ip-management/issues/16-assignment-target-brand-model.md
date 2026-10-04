# 16: IP 清單指派對象顯示「描述(廠牌 型號)」

**What to build:** IP 管理清單的「指派對象」第一行改為顯示「資產描述(廠牌 型號)」（廠牌／型號缺者省略，皆缺僅顯示描述），讓同名或同類資產可辨識。

**Blocked by:** None (can start immediately)

**Status:** done

- [x] 後端指派對象（IP 清單列）補 `asset_brand`／`asset_model` 欄位（比照票 15 的 `asset_property_no` 做法；查詢帶出 assets 欄位）
- [x] 第一行格式：`描述(廠牌 型號)`；只有廠牌→`描述(廠牌)`、只有型號→`描述(型號)`、皆缺→僅 `描述`
- [x] 第二行維持介面名稱／MAC（＋hostname）；v4／v6 一致
- [x] 後端整合測試涵蓋各缺值組合；前端 typecheck／lint 全綠

## Comments

實作完成（commit 訊息：`16 IP 清單指派對象顯示：描述(廠牌 型號)（後端＋前端）`）。

- 後端：
  - `backend/src/assignments.rs`：`ListedAssignment` 與 `IpAssignment` 新增 `asset_brand`／`asset_model`（`Option<String>`）；`list_for_subnet` 查詢帶出 `assets.brand`／`assets.model`，`ListedAssignment::target()` 一併帶出，IP 清單指派對象自此含廠牌／型號。
  - `AssignmentTargetRow`／`fetch_target` 同步帶出廠牌／型號；`assigned_elsewhere_error` 的 `details` 新增 `asset_brand`／`asset_model`（比照票 15 的 `asset_property_no` 做法，維持指派對象摘要欄位一致）。`AssetAssignment`（資產詳情唯讀列表）與匯入報告 `AssignmentTarget` 未動：非票面顯示所需，不擴大範圍。
- 前端：
  - `api/ips.ts`：`IpAssignmentTarget` 新增 `asset_brand`／`asset_model`（`string | null`）。
  - `IpListPage.vue`：新增 `assignmentTargetLabel()`，指派對象第一行組「描述(廠牌 型號)」；只有廠牌→`描述(廠牌)`、只有型號→`描述(型號)`、皆缺→僅描述。第二行（介面名稱／MAC＋hostname）與 v4／v6 共用 cell 維持不變。
- 測試：
  - `backend/tests/ips.rs`：新增 `rows_report_assignment_asset_brand_and_model`，涵蓋四種缺值組合（皆有／只有廠牌／只有型號／皆無），v4 與 v6 皆驗；既有 `rows_report_assignment_location` 補「未填廠牌／型號為 null」斷言。
  - `backend/src/ips.rs`：新增單元測試 `assignment_target_carries_asset_brand_and_model`；既有 `assigned_rows_report_status_and_target` 補 null 斷言；`ips.rs`／`conflicts.rs` 測試 helper 的 `ListedAssignment` 補欄位。
  - 驗收全綠：`pnpm test`（71 單元＋95 整合，共 166）、`cargo fmt --check`、`pnpm --filter frontend typecheck`、`pnpm lint:check`。
- 與規格差異／取捨：
  - 指派對象欄排序（票 14）仍以資產描述為鍵、不變（見 spec §4.3）；廠牌／型號僅供顯示。
  - 對話框（票 15）維持「財產編號(描述)」格式，未加入廠牌／型號（見 spec §4.3）。
  - 移轉錯誤 `details` 新增 `asset_brand`／`asset_model` 僅為欄位一致性（比照票 15）；前端顯示格式不變、未使用該兩欄。
- 實作 commit：`06271a6`（`16 IP 清單指派對象顯示：描述(廠牌 型號)（後端＋前端）`）
