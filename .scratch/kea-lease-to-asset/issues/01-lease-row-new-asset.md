# 01: 租約列「新增資產」入口與「保留」篩選

**What to build:** 租約清單新增「保留」篩選（保留位址／非保留位址，對應 `is_reservation`）與每列「新增資產及指派 IP」按鈕：保留列停用（仍顯示）、未受管（`subnet_cidr=null`）與缺 IP 停用；點擊開啟既有「新增資產」對話框並預填描述（租約 hostname）與一筆介面（`eth0`＋租約 MAC），不預填 IP、不建立指派，儲存後重載租約清單。指派一律沿用資產編輯的既有流程（要保留租約 IP 須先自 DHCP 位址池移出）。詳見 `.scratch/kea-lease-to-asset/spec.md`。

**Blocked by:** None

**Status:** done

- [x] 篩選列新增「保留」下拉（保留位址／非保留位址；clearable；與搜尋、狀態 AND）
- [x] 操作欄按鈕「新增資產及指派 IP」：保留列停用＋tooltip「已是保留（已有資產設定）」、未受管與缺 IP 停用；declined／released／expired-reclaimed 只要 IP＋MAC 具備即啟用
- [x] 點擊開啟 `AssetFormDialog`（新增模式）並預填：描述＝hostname（無則空）、一筆介面 eth0＋MAC；不預填 IP、不建立指派、不自動跳轉
- [x] `AssetFormDialog` 新增選填預填 props；未提供時行為不變（編輯／指派流程不回歸）
- [x] 儲存後重載租約清單、停留原頁
- [x] README 租約清單描述微調
- [x] `pnpm --filter frontend typecheck`、`pnpm lint:check` 綠；`cargo test`（後端未動）回歸綠

## 注意

- 不採「放行池內保留」：後端指派規則（`backend/src/assignments.rs::validate_address` 等）**不動**。
- 不預填 IP、不自動建立指派、不自動跳轉；不做 MAC 反查既有資產。
- 租約頁不呼叫 Kea 寫入（`is_reservation` 等欄位沿用現行 API）。
- 與票 02 同動 `frontend/src/pages/KeaLeasesPage.vue`（欄位與 `pagination` 定義保持一致）。

## Comments

實作完成（主實作 commit `c80950f`）：

- `KeaLeasesPage.vue`：篩選列新增「保留」`q-select`（保留位址／非保留位址；clearable、`emit-value`＋`map-options`；與搜尋／狀態 AND）；表格尾欄「操作」與「新增資產及指派 IP」按鈕（一律顯示；停用＋tooltip 四條件：已是保留／未受管／缺 IP／缺 MAC〔防禦〕）；點擊以該列租約預填（描述＝hostname、eth0＋MAC）並開啟 `AssetFormDialog`（`:asset="null"`）；`@saved` 重載租約清單。
- `AssetFormDialog.vue`：新增選填 props `prefillDescription`／`prefillInterface`（僅新增模式套用；未提供行為不變），介面草稿沿用現行結構（`id = null`）。
- `README.md`：租約清單描述改為「唯讀檢視，可自租約建立資產」。
- 驗收：`pnpm --filter frontend typecheck` 綠；`pnpm lint:check` 綠（65 檔）；`pnpm build:frontend` 成功；`cargo test`（後端未動）406 passed／0 failed／6 ignored。
- 未做（依 spec 範圍）：自動指派、指派對話框預填 IP、MAC 反查既有資產；瀏覽器人工檢核待實機確認。
