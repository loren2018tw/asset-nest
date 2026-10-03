# 02: 資產介面管理

**What to build:** 資產編輯對話框內的介面子編輯器(新增/刪除),以及資產詳情對介面的顯示。

**Blocked by:** 01 資產管理

**Status:** done

- [x] 資產對話框可新增/刪除介面;介面欄位:名稱、MAC、備註
- [x] MAC 空白時名稱必填(阻擋儲存);有 MAC 時名稱選填
- [x] MAC 正規化為小寫冒號格式儲存;全系統重複 MAC 顯示提示但不阻擋
- [x] 刪除介面有確認(此階段尚無指派,影響數量於 08 補上)
- [x] 後端整合測試涵蓋介面 CRUD 與名稱門檻規則

## Comments

實作完成（commit 訊息：`02 資產介面管理：…`）。

- 後端：migration `0003_interfaces.sql` 建立 `interfaces` 表（`asset_id` FK ON DELETE CASCADE、`CHECK`：MAC 非空或名稱非空非空白、`asset_id`／`mac` 索引）；`src/interfaces.rs` 領域模組（驗證、MAC 正規化、重複 MAC 警示、存取）；API 掛在 `/api/v1`：
  - `POST /assets/{id}/interfaces`（201；資產不存在回 404）
  - `PATCH /interfaces/{id}`、`DELETE /interfaces/{id}`
  - `GET /assets/{id}` 擴充為資產欄位＋`interfaces` 陣列（依建立順序）
  - MAC 正規化：接受冒號／連字號／點號／無分隔的 12 位十六進位，儲存為小寫冒號格式；其餘格式回 400 `validation_error`（`details.field = "mac"`）
  - 名稱門檻：MAC 空白（含未提供、空字串、全空白）時名稱必填，否則 400（`details.field = "name"`）；PATCH 以「與既有值合併後的最終狀態」判斷，避免清除 MAC 後留下無名稱介面
  - 重複 MAC：正規化後全系統比對（排除自身），僅於回應加 `warnings: [{code: "duplicate_mac", message}]`，不阻擋；此 `warnings` 機制供後續票（如 07 的語意衝突）沿用
  - PATCH 語意與資產一致：未提供欄位維持原值、`null`／空字串清除
- 前端：`AssetFormDialog.vue` 加入介面子編輯器（新增／刪除、名稱／MAC／備註；刪除有確認對話框）。介面以本地草稿管理，隨資產儲存同步：新增資產時先建立資產、再逐筆新增／PATCH／DELETE 介面；MAC 重複警示以 `$q.notify` 顯示。`GET /assets/{id}` 於編輯模式載入既有介面；`api/interfaces.ts` 新增介面 API 與 `Warning` 型別，`api/assets.ts` 新增 `AssetDetail`／`fetchAsset`
- 測試：`backend/tests/interfaces.rs` 5 個整合測試（記憶體 SQLite＋`sqlx::migrate!`），涵蓋介面 CRUD、404 情境、MAC 空⇒名稱必填（含 PATCH 合併狀態與 DB CHECK 雙保險）、MAC 正規化（五種輸入格式與四種錯誤格式）、重複 MAC 僅警示不阻擋（含排除自身）、刪除資產連動刪除介面；`interfaces.rs` 另有 4 個 MAC 正規化／名稱門檻單元測試。`backend/tests/assets.rs` 的 GET 詳情斷言配合擴充調整
- 驗證：`pnpm test`（8 單元＋15 整合全綠）、`pnpm --filter frontend typecheck`、`pnpm lint:check`、`cargo fmt` 全綠；另以真實 SQLite 啟動後端煙霧測試完整流程（新增／警示／阻擋／詳情／編輯／連動刪除）
- 與規格差異：規格 §5 未列 `GET /assets/{id}` 的介面警示欄位，重複 MAC 僅在新增／編輯回應提供 `warnings`；已指派 IP 唯讀顯示留待後續票
