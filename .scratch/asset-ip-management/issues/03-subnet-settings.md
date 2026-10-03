# 03: 網段設定

**What to build:** 網段設定頁與 CRUD;v4/v6 單一地址族網段,含名稱、備註、gateway、pool 多段(v4)、Kea subnet-id(v4)。結構性驗證阻擋非法資料。

**Blocked by:** None (can start immediately)

**Status:** done

- [x] 可建立、編輯、刪除網段;CIDR 為必填且單位址族
- [x] v4 可設多段 pool 與 kea_subnet_id(唯一);v6 不接受 pool 與 kea_subnet_id
- [x] 結構驗證阻擋並有明確錯誤:與既有網段重疊(含完全相同、嵌套)、gateway 不在 CIDR 內、pool 不在 CIDR 內、pool 段間重疊
- [x] 網段名稱選填、不強制唯一
- [x] 網段列表顯示名稱、CIDR 與地址族
- [x] 後端整合測試涵蓋重疊與各項結構驗證

## Comments

實作完成（commit 訊息：`03 網段設定：網段 CRUD、結構驗證與列表頁（後端＋前端）`）。

- 後端：migration `0004_subnets.sql` 建立 `subnets`（`cidr` UNIQUE、`kea_subnet_id` UNIQUE）與 `subnet_pools`（`subnet_id` FK ON DELETE CASCADE）；`src/subnets.rs` 領域模組（CIDR 正規化與結構驗證、pool 驗證、重疊／kea_subnet_id 衝突檢查、存取）；`src/api/subnets.rs` 掛載 `/api/v1`：
  - `GET /subnets`（列表摘要：名稱、CIDR、地址族）、`POST /subnets`（201）
  - `GET /subnets/{id}`（供編輯對話框載入完整內容含 pools；規格 §5 未列，比照 `GET /assets/{id}` 模式）、`PATCH /subnets/{id}`、`DELETE /subnets/{id}`（204；非空防護見票 08）
  - `ApiError` 新增 `detail()`：結構錯誤附 `details.field` 與 `details.conflict`（衝突網段的 id／cidr／name）
- 結構驗證（阻擋儲存，見 ADR-0006／spec §3.1）：
  - CIDR 必填、須合法；host bits 收斂為網路地址後儲存（`trunc()`）
  - 與既有網段重疊一律 400：完全相同、嵌套（新在外／新在內）、未對齊輸入正規化後重疊；訊息含衝突網段名稱與 CIDR
  - kea_subnet_id 重複 400（應用層檢查＋DB UNIQUE 雙保險）；名稱選填、不強制唯一
  - gateway 須在 CIDR 內（跨地址族視為不在內）；PATCH 以「與既有值合併後的最終狀態」驗證，未提供 gateway 而僅縮小 CIDR 導致出界亦會阻擋
  - v4 可多段 pool：每段端點須為 IPv4、須在 CIDR 內、起點不得大於終點、段間不得重疊（含共用端點）；v6 一律拒絕 pool 與 kea_subnet_id，PATCH v4→v6 時殘留欄位亦以合併後狀態阻擋
- 前端：`/ips` 由佔位頁改為 `SubnetsPage.vue`（列表顯示名稱、CIDR、地址族；新增／編輯／刪除，刪除有確認）；`SubnetFormDialog.vue`（CIDR／名稱／gateway／Kea subnet-id／備註＋pool 多段編輯；v6 自動隱藏 pool 與 kea 欄位）；`api/subnets.ts`；新增 `utils/cidr.ts` 供對話框「即時顯示重疊等結構錯誤」（見 spec §4.2）：重疊、gateway 出界、pool 出界／段間重疊即時以欄位錯誤與 banner 呈現，有即時錯誤時不送請求（後端仍為權威；解析失敗不猜測，交由後端回報）
- 測試：`backend/tests/subnets.rs` 7 個整合測試（記憶體 SQLite＋`sqlx::migrate!`）涵蓋 CRUD／404／CIDR 必填與正規化／重疊各形式（含編輯排除自身、v6 重疊）／gateway（含僅縮小 CIDR 的合併狀態）／pool 範圍與段間重疊／v6 限制（含 v4→v6 殘留欄位）／kea_subnet_id 唯一（含 DB UNIQUE 直寫驗證）；`subnets.rs` 內 6 個單元測試（正規化、必填、gateway、v6 限制、pool 規則、地址族判斷）
- 驗收：`cargo test`（14 單元＋22 整合全綠）、`cargo fmt --check`、`pnpm typecheck`、`pnpm lint:check` 全綠
- 與規格差異：列表「已用／總數／衝突數」依票 07、刪除非空網段的防護依票 08 尚未實作；`GET /subnets/{id}` 為規格 §5 未列之新增（供編輯對話框載入 pools）
- 實作 commit：`bafc6b9`（`03 網段設定：網段 CRUD、結構驗證與列表頁（後端＋前端）`）
