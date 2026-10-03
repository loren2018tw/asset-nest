# 08: 刪除連動與防護

**What to build:** 刪除行為的確認資訊與防護:資產/介面刪除顯示連動影響數量後執行;有指派的網段不可刪除。

**Blocked by:** 05 IP 指派(手動／保留)、06 v6 位址登錄制

**Status:** done

- [x] 刪除資產前顯示將連動刪除的介面數、指派數與其中保留數;確認後一併刪除
- [x] 刪除介面前顯示影響的指派數;確認後一併刪除
- [x] 網段有任何指派(含 v6)即不可刪除,錯誤訊息顯示數量;無指派的網段可刪
- [x] 後端整合測試涵蓋連動刪除與網段防護

## Comments

實作完成（commit 訊息：`08 刪除連動與防護：資產/介面連動刪除、非空網段防護（後端＋前端）`）。

- 後端：
  - 連動刪除沿用既有 DB 外鍵（`interfaces.asset_id`、`ip_assignments.interface_id` 皆 `ON DELETE CASCADE`），未改 schema；`assets::delete`／`interfaces::delete` 邏輯不變，改以整合測試鎖定完整連動。
  - 新增 `assignments::count_for_subnet`（static＋reservation 筆數）。
  - 新增 `subnets::ensure_deletable`：`DELETE /subnets/{id}` 先檢查網段是否存在（不存在回 404），再檢查指派筆數；>0 回 409 `{error: "conflict", message, details: {assignments: N}}`，訊息含網段描述與數量（如「網段「辦公區」（10.0.0.0/24）尚有 2 筆指派（含保留），不可刪除；請先取消所有指派」）。`ApiError` 新增 `conflict` 建構子（409）。
  - 狀態碼取捨：規格僅要求結構化錯誤、未定狀態碼；刪除非空網段是「請求本身合法但與資源目前狀態衝突」，選 409（而非既有結構驗證的 400 `validation_error`），錯誤碼 `conflict`；格式仍沿用 `{error, message, details?}`。
- 前端：
  - `AssetsPage.vue`：刪除確認前先 `GET /assets/{id}`（票 05 已含 interfaces／assignments），顯示「將刪除 N 個介面、M 筆指派（含 K 筆保留）」再執行；詳情讀取失敗時提示錯誤且不進入刪除流程。未新增後端統計端點。
  - `AssetFormDialog.vue`：介面刪除確認依對話框已載入的 `assignments` 統計該介面指派數，有指派時顯示「將連動刪除 M 筆指派。」；未儲存的新介面不受影響。
  - `SubnetsPage.vue`：不需改動——既有 `catch` 會以 `$q.notify` 顯示後端 409 的 `message`（含數量），刪除失敗時不刷新清單、資料保留。
- 測試：新增 `backend/tests/delete_linkage.rs` 4 個整合測試——刪除含 2 介面＋v4 手動／保留＋v6 登錄的資產後，介面與指派全消失、對照組資產不受影響、位址回可用、v6 清單清空；刪除介面連動刪除其跨網段指派（v4／v6 各一）、另一介面的保留不受影響；v4（2 筆，含保留）與 v6（1 筆）非空網段刪除回 409 且 `details.assignments` 與訊息數量正確、網段未誤刪，取消指派後可刪；有 pool 但無指派的網段可刪（pools 連動）且重複刪除回 404。
- 驗收：`cargo test`（39 單元＋51 整合全綠）、`cargo fmt --check`、`pnpm typecheck`、`pnpm lint:check` 全綠；另以真實伺服器＋暫存 SQLite smoke（v4／v6 非空刪除回 409 含數量；刪除資產後 v4 位址回可用、v6 清單清空；空網段可刪）。
- 與規格差異／取捨：
  - 網段防護狀態碼採 409（規格未指定，見上）。
  - 資產刪除的連動數量不新增後端端點，由前端讀既有詳情計算（票檔允許；`GET /assets/{id}` 已含 interfaces 與 assignments）。
  - 介面刪除實際發生在資產對話框儲存時（既有流程），確認對話框顯示的指派數以對話框開啟時載入的詳情為準。
- 實作 commit：`cb19ec0`（`08 刪除連動與防護：資產/介面連動刪除、非空網段防護（後端＋前端）`）
