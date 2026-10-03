# 01: 資產管理

**What to build:** 資產管理縱切:側欄導覽(資產管理、IP 管理)、資產清單頁與新增/編輯對話框、位置既有值建議、屆齡徽章、刪除。依 `.scratch/asset-ip-management/spec.md` 與 `CONTEXT.md` 詞彙。

**Blocked by:** None (can start immediately)

**Status:** done

- [x] 可建立、編輯、刪除資產;描述與位置必填(缺漏有明確錯誤)
- [x] 清單欄位:財產編號、描述、位置、廠牌、型號、備註、屆齡徽章;支援關鍵字搜尋(財產編號/描述/設備序號/廠牌/型號/備註)、位置與廠牌篩選、分頁
- [x] 位置輸入時列出既有位置建議(既有值去重、不分大小寫)
- [x] 購置日期＋年限早於今日的資產顯示屆齡徽章,且不影響任何操作
- [x] 設備序號選填、可搜尋、重複僅提示不阻擋
- [x] id 為資料庫自增,不出現在 UI
- [x] 後端整合測試涵蓋 CRUD、必填驗證、搜尋與篩選

## Comments

實作完成（commit `bf144dc`）。

- 後端：migration `0002_assets.sql` 建立 `assets` 表；`src/assets.rs` 領域模組（驗證、屆齡、查詢）；API 掛在 `/api/v1`：
  - `GET/POST /assets`、`GET/PATCH/DELETE /assets/{id}`、`GET /locations`、`GET /brands`
  - 清單：`q` 跨六欄位子字串（LIKE 萬用字元已轉義）、`location`／`brand` 不分大小寫完全符合、`device_serial` 精確查找（重複提示用）、`page`／`per_page`（預設 50、上限 200）；預設排序描述升冪
  - 屆齡由後端計算 `expired`：`purchase_date + lifespan_years < 今天`（chrono、Local 時區；2/29 加年退至 2/28）
  - PATCH：未提供欄位不變、`null`／空字串清除選填欄位；描述／位置一旦提供即必填
- 前端：側欄新增「資產管理」「IP 管理」；`/assets` 清單頁（搜尋、位置／廠牌篩選、伺服器端分頁、屆齡徽章、列編輯／刪除）與 `AssetFormDialog.vue`（位置既有值建議、設備序號重複僅提示）；`/ips` 為後續階段佔位頁；API client 擴充 POST／PATCH／DELETE 並採用後端 `{error,message}` 訊息
- 測試：`backend/tests/assets.rs` 7 個整合測試（記憶體 SQLite＋`sqlx::migrate!`），涵蓋 CRUD、必填／格式驗證、關鍵字搜尋（含 `%` 轉義）、位置／廠牌篩選、分頁與排序、位置／廠牌去重、設備序號重複、屆齡邊界與「不影響操作」
- 驗證：`pnpm test`、`pnpm --filter frontend typecheck`、`pnpm lint:check`、`cargo fmt --check` 全綠；另以真實 SQLite 檔案啟動後端煙霧測試
- 與規格差異：規格 §5 未列 `/brands`，為廠牌篩選選單而新增（與 `/locations` 同模式）；`GET /assets/{id}` 尚未含 interfaces／已指派 IP，留待票 02 擴充
