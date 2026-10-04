# 10: 資產端指派 IP（含移轉）

**What to build:** 從資產清單與資產編輯對話框的介面清單直接指派 IP；輸入 IP 後若已指派給其他介面，提示並在確認後將指派移轉到目前介面（v4/v6 皆支援，原子取消＋重新指派）。決策另立 ADR-0007。

**Blocked by:** 09 連線主機 MAC 參考（新增介面表單沿用其 MAC 提示）

**Status:** done

- [x] 資產管理列表每列新增「指派 IP」按鈕；資產編輯對話框的介面列亦新增「指派 IP」按鈕（未儲存的資產／介面不顯示）
- [x] 指派對話框：輸入 IP、選介面（或當場新增介面：名稱／MAC，含票 09 的連線主機 MAC 提示）、用途（手動／保留；無 MAC 時保留停用；保留可填 hostname）
- [x] IP 已指派給其他介面時，提示目前指派對象（資產描述／位置、介面名稱／MAC），確認後移轉到目前介面
- [x] 移轉為原子操作（單一交易內取消＋重新指派），不留歷程（見 ADR-0005）
- [x] 結構規則阻擋：保留需介面有 MAC；同一介面在同一網段至多一位址；位址須落在某網段內（v4 另須為 host 且非池內）；介面須屬於該資產
- [x] v6：登錄即指派；已登錄給其他介面時同樣提示移轉；用途固定手動設定
- [x] 後端整合測試涵蓋指派、移轉（v4/v6）、各結構規則與不信任情境（介面不屬資產、位址不在網段內）

## Comments

實作完成（commit 訊息：`10 資產端指派 IP：清單與介面列入口、確認後原子移轉（v4/v6）（後端＋前端）`）。

- 後端：
  - `backend/src/subnets.rs`：新增 `find_by_address`，以 `IpNet::contains` 掃描網段反推所屬網段（網段不重疊，至多一個）；找不到回 `None`。
  - `backend/src/assignments.rs`：新增 `AssetAssignmentInput`（`address`／`interface_id`／`purpose`／`hostname`／`transfer`，`transfer` 預設 `false`）與 `assign_for_asset`。流程：資產存在 → 介面存在且屬於資產 → 由位址找網段（無 → 400 `no_subnet`）→ 結構規則（保留需 MAC、v6 固定 static、目標介面同網段已有其他位址）→ 既有位址判斷（同介面＝更新；其他介面且 `transfer=false` 回 400 `address_assigned_elsewhere` 附目前對象；`transfer=true` 標記移轉）→ 位址驗證（未指派時驗 v4 host／非池內、v6 在 CIDR 內；更新／移轉沿用既有位址原則不重驗出界／落池）→ 寫入。移轉由 `transfer_atomically` 於單一交易內 `DELETE` 舊列＋`INSERT` 新列；`assigned_elsewhere_error` 的 `details` 附 `subnet_id`、`subnet_cidr`、`asset_id`、`asset_description`、`asset_location`、`interface_id`、`interface_name`、`mac`。
  - `backend/src/api/asset_assignments.rs`（新增）：`PUT /api/v1/assets/{id}/assignments`，回應既有 `Assignment` 欄位＋`warnings`＋`transferred`；`backend/src/api/ips.rs` 抽出 `warnings_for_address` 供兩端點共用（警示機制不變）。
  - 既有 `PUT /subnets/{id}/ips/{address}/assignment` 未動：已指派給其他介面仍直接阻擋（回歸測試涵蓋）。
- 前端：
  - `frontend/src/api/client.ts`：`ApiError` 增加 `details`（錯誤 body 的 `details`；原 `{error, message}` 解析保持相容）。
  - `frontend/src/api/assignments.ts`（新增）：`assignFromAsset(assetId, input)`，回傳含 `transferred` 與 `warnings`。
  - `frontend/src/components/AssignIpDialog.vue`（新增）：開啟時 `fetchAsset` 載入介面；`fixedInterface` 存在時隱藏介面選擇，否則下拉（僅一個介面預選）；IP 以 `parseAddress` 驗格式（v4／v6）、當場新增介面（名稱／MAC＋`PeerMacHint`）、用途（無 MAC 時保留停用；v6 固定手動不顯示選項）、保留時 hostname。送出先 `transfer: false`；`details.reason === "address_assigned_elsewhere"` 時以 `$q.dialog` 顯示目前對象，確認後 `transfer: true` 重試；成功依 `transferred` 通知「已指派／已移轉並指派」、逐筆 warning、emit `saved` 並關閉。
  - `frontend/src/pages/AssetsPage.vue`：列操作新增 `add_link`「指派 IP」按鈕；`@saved` 重新載入清單。
  - `frontend/src/components/AssetFormDialog.vue`：已儲存介面列（`row.id !== null`）顯示「指派 IP」按鈕，開啟時以該介面為 `fixedInterface`；`@saved` 更新唯讀「已指派 IP」區塊。
- 測試：
  - 新增 `backend/tests/asset_assignments.rs`（6 個整合測試）：v4 新指派＋資產詳情可見＋同介面更新用途（id 不變、`transferred: false`）；已指派其他介面 `transfer` 未給／false 皆擋且 details 完整、原指派不變，`transfer: true` 移轉成功（`transferred: true`、列 id 改變、舊介面消失、新介面存在、同網段其他列不受影響）；結構規則（保留無 MAC、目標介面同網段已有位址、介面不屬資產、位址不在任何網段、v4 池內、network／broadcast、不存在資產 404／介面 400、非法位址）；v6（新登錄、network 位址可登錄、擋／移轉、用途非 static 擋）；`warnings` 沿用衝突機制（DuplicateHwAddress）；既有網段端點對「已指派其他介面」仍阻擋的回歸。
  - `assignments.rs` 單元測試新增 2 個（`AssetAssignmentInput` 驗證與 `transfer` 預設 false）。
  - 驗收全綠：`cargo test`（45 單元＋59 整合，共 104）、`cargo fmt --check`、`pnpm typecheck`、`pnpm lint:check`、`pnpm build`。
- 與規格差異／取捨：
  - 資產不存在回 404（票文「驗證資產存在」未指定狀態碼；比照 `GET /assets/{id}` 慣例）。
  - `AssetFormDialog` 的 `@saved` 只重載 `assignments`（唯讀區塊），不重載介面草稿：避免把使用者尚未儲存的介面編輯丟棄；新介面建立在此入口因 `fixedInterface` 固定而不可用，故不影響介面清單同步。
  - `spec.md` §5 增補本端點列；§4.1 原文「指派一律在 IP 管理頁操作」保留未改，新入口與決策由 ADR-0007 記錄。
  - 位址若因網段縮小而「出界」，將不屬於任何網段，資產端無法以此路徑移轉（回 `no_subnet`）；此已知邊界記於 ADR-0007。
- 實作 commit：`447bbec`（`10 資產端指派 IP：清單與介面列入口、確認後原子移轉（v4/v6）（後端＋前端）`）
