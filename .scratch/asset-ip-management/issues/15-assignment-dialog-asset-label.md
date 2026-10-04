# 15: 指派對話框資產顯示「財產編號(描述)」

**What to build:** IP 端指派／編輯對話框與資產端「指派 IP」對話框，凡是顯示資產的地方一律用「財產編號(描述)」格式，讓同名資產可辨識。

**Blocked by:** None (can start immediately)

**Status:** done

- [x] 格式：`財產編號(描述)`；財產編號為空時只顯示描述
- [x] 適用：IP 端對話框的搜尋選項主標題、選取值、編輯模式唯讀「資產」欄、移轉確認訊息；資產端「指派 IP」對話框副標題
- [x] 搜尋選項的 caption 保留位置協助辨識
- [x] 編輯已指派 IP 時資產／介面維持唯讀（不得新增直接更換目標的入口）
- [x] 前端 typecheck／lint 全綠

## Comments

實作完成（commit 訊息：`15 指派對話框資產顯示：財產編號(描述)（後端＋前端）`）。

- 後端：
  - `backend/src/assignments.rs`：`ListedAssignment` 與 `IpAssignment` 新增 `asset_property_no`（`Option<String>`）；`list_for_subnet` 查詢帶出 `assets.property_no`，`ListedAssignment::target()` 一併帶出，IP 清單指派對象自此含財產編號。`AssignmentTargetRow`／`fetch_target` 同步帶出財產編號，`assigned_elsewhere_error` 的 `details` 新增 `asset_property_no`（供資產端移轉確認訊息組「財產編號(描述)」）。
  - `AssetAssignment`（資產詳情唯讀列表）與匯入報告 `AssignmentTarget` 未動：非票面顯示所需，不擴大範圍。
- 前端：
  - 新增 `frontend/src/utils/assetLabel.ts`：`assetLabel(propertyNo, description)`＝「財產編號(描述)」；財產編號 null／空白僅顯示描述。兩個對話框共用。
  - `AssignmentDialog.vue`：搜尋選項主標題與選取值改用 `assetOptionLabel`（caption 維持位置）；編輯模式唯讀「資產」欄改用 `assetLabel`。已指派列資產／介面維持唯讀，未新增任何更換目標入口（換目標仍為取消後重新指派，見票 05）。
  - `AssignIpDialog.vue`：副標題改「財產編號(描述)（位置）」；移轉確認訊息以 `assetLabel` 組目前指派對象（保留位置）與介面資訊；`parseAssignedElsewhere`／`AssignedElsewhere` 新增解析 `asset_property_no`。
  - `api/ips.ts`：`IpAssignmentTarget` 新增 `asset_property_no: string | null`。
- 測試：
  - `backend/src/ips.rs`：新增單元測試 `assignment_target_carries_asset_property_no`；既有 `assigned_rows_report_status_and_target` 補無財產編號為 null 斷言；`ips.rs`／`conflicts.rs` 測試 helper 的 `ListedAssignment` 補欄位。
  - `backend/tests/assignments.rs`（票 05 檔）：IP 清單回應含 `asset_property_no`（P-001）、無財產編號為 null；新增 `create_asset_with_property_no` helper。
  - `backend/tests/asset_assignments.rs`（票 10 檔）：移轉錯誤 `details.asset_property_no`（有值 P-100、v6 無值為 null）；移轉前後 IP 清單財產編號隨目前指派對象（移轉後對象無編號即 null）。
  - 驗收全綠：`pnpm test`（70 單元＋94 整合，共 164）、`cargo fmt --check`、`pnpm --filter frontend typecheck`、`pnpm lint:check`；另 `pnpm --filter frontend build` 成功。
- 與規格差異／取捨：
  - IP 端編輯對話框無移轉流程（票 05 定案：換目標＝先取消再重新指派），故「移轉確認訊息」實際僅存在於資產端 `AssignIpDialog`，格式套用於該處。
  - 編輯模式唯讀「資產」欄由原「描述（位置）」改為純「財產編號(描述)」：位置資訊保留於搜尋選項 caption、`AssignIpDialog` 副標題與移轉訊息，以及 IP 清單「位置」欄。
- 實作 commit：`077b108`（`15 指派對話框資產顯示：財產編號(描述)（後端＋前端）`）
