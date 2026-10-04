# 01: 匯入前置重構——領域程序可於交易內重用

**What to build:** 讓 CSV 匯入能重用既有的領域驗證與寫入，而不是複製規則或 SQL：MAC 正規化與位址結構驗證（host 範圍／pool／地址族）開放給匯入模組使用；資產與介面的建立抽出「可傳入交易連線」的原語，既有建立函式改為薄包裝。既有 API 行為與回應完全不變。

**Blocked by:** None (can start immediately)

**Status:** done

- [x] MAC 正規化與位址結構驗證程序開放 crate 內匯入模組使用；規則與錯誤文案不變
- [x] 資產／介面建立抽出可接受交易連線的原語；既有建立路徑改為呼叫原語、使用連線池
- [x] 現有 API 回應與錯誤格式零變化；`cargo test` 全綠（單元＋整合）
- [x] 新增測試：原語於交易內建立後可被同交易讀取；交易回滾後不留下資料

## Comments

實作完成（commit `415607d`）。

- `assets::insert_asset(&mut SqliteConnection, ValidAsset) -> i64`、`interfaces::insert_interface(&mut SqliteConnection, asset_id, ValidInterface) -> i64`：可於交易內使用的建立原語；`create` 改為「取連線→原語→歸還→fetch」薄包裝，對外簽章、回應與錯誤行為零變化。
- `interfaces::normalize_mac`、`assignments::validate_address` 改為 `pub(crate)`，規則與錯誤文案不變；`validate_address` 註記匯入將以 `existing = false` 情境重用。
- 新增 4 個單元測試（交易內可見、回滾不留資料；資產與介面各 2），測試 pool 比照 `backend/tests/` 以 `sqlite::memory:`＋migrations 建立。
- 驗證：`cargo fmt --check`、`cargo test` 全綠（49 單元＋65 整合、0 failed）。

