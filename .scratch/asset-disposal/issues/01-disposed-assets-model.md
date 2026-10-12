# 01 — disposed_assets 資料模型與領域邏輯

Status: done
Blocked by: —

## 目標

實作 `spec §3`：migration、`backend/src/disposed_assets.rs` 型別、驗證與查詢。此票只做資料層與領域邏輯，不動 API handler 與前端（後續票）。

## 範圍

- 新增 `backend/migrations/0012_disposed_assets.sql`（schema 照 spec §3；含 `idx_disposed_assets_disposal_date`）。
- 新增 `backend/src/disposed_assets.rs`（比照 `assets.rs`／`lendings.rs` 風格）：
  - `COLUMNS`、`DisposedAssetRow`(FromRow)、`pub struct DisposedAsset`（`tags: Vec<String>`）、`DisposeInput`、`ValidDispose`。
  - `DisposeInput::validate(self, today: NaiveDate)`：必填（field `disposal_date`）、嚴格 `YYYY-MM-DD`（4-2-2 位數字＋`NaiveDate` 日曆檢查，比照 `lendings::validate_due_date`）、不可為未來。
  - `dispose(pool, asset_id, valid)`：單一交易——讀 `assets` 整列（不存在回 `Ok(None)`；`tags` 保持 JSON 字串）→ INSERT 複製全欄位＋`asset_id`＋`disposal_date` → `DELETE FROM assets`（CASCADE 清介面／指派／借還）→ 回新列。
  - `list(pool, q, page, per_page)`：LIKE 子字串比對 `property_no／description／location／device_serial／brand／model／note`；排序 `disposal_date DESC, id DESC`；回 items 與 total。
  - `restore(pool, id)`：單一交易——讀報廢列（不存在回 `Ok(None)`）→ `INSERT INTO assets`（id 用 `asset_id`、`created_at` 沿用、`updated_at` 為當下）→ DELETE 報廢列 → 提交後以 `assets::get` 回傳。
- 將 `assets::escape_like` 提升為 `pub(crate)` 供 `list` 共用。
- `backend/src/lib.rs` 加 `pub mod disposed_assets;`（`db` 與 `import` 之間，字母序）。
- 單元測試（`:memory:`＋migrations，照 spec §3 清單）：
  - 驗證：缺漏／空白、`2026/1/1`、`1-2-3`、明天、今天、過去。
  - dispose：全欄位資產＋介面＋指派＋已歸還借還 → 搬移後 `assets` 無、欄位一致（`created_at` 相同）、介面／指派／借還列消失。
  - restore：原 id、`created_at` 保留、`updated_at` 更新、`tags` 還原、報廢列消失、無介面與指派。
  - `list`：搜尋、`%` 字面、分頁 total、排序。

## 驗收

- `cargo test --manifest-path backend/Cargo.toml --lib disposed_assets`
- `cargo fmt --manifest-path backend/Cargo.toml -- --check`
- `cargo test --lib` 既有測試全綠。

## 注意

- 風格比照現有模組：繁體中文註解、snake_case、`Option<String>` 欄位。
- 不要 `git commit`；不要動 `.scratch/` 內其他票；不改 API、不改前端。
- 不改既有 migration（sqlx 有 checksum）。

## Comments

### 2026-10-12 完成紀錄（agent）

- 新增 `backend/migrations/0012_disposed_assets.sql`：schema 照 spec §3（`assets` 複製＋`asset_id UNIQUE`＋`disposal_date`；`created_at`／`updated_at` 複製原值），含 `idx_disposed_assets_disposal_date`，繁體中文註解。
- 新增 `backend/src/disposed_assets.rs`：
  - 型別：`COLUMNS`、`DisposedAssetRow`（FromRow，`tags: String`）、`DisposedAsset`（Serialize，`tags: Vec<String>`）、`DisposeInput`（`disposal_date: Option<String>`）、`ValidDispose`。
  - 驗證：`DisposeInput::validate(today)`——必填（缺漏／空白 →「報廢日期為必填」）、嚴格 `YYYY-MM-DD`（4-2-2 位數字＋`NaiveDate` 日曆檢查，比照 `lendings::validate_due_date`；`2026/1/1`、`1-2-3`、`2026-13-01` 皆格式錯）、不可為未來（今天可、可回填過去）；field 一律 `disposal_date`。
  - `dispose`：單一交易讀 `assets` 整列 → INSERT 複製全欄位＋`asset_id`＋`disposal_date` → DELETE `assets`（FK CASCADE 清介面／指派／借還）→ 回新列；資產不存在回 `Ok(None)`（交易回滾、不留變更）。
  - `list`：LIKE 子字串比對七欄（`property_no／description／location／device_serial／brand／model／note`；`%`／`_` 轉義為字面）、排序 `disposal_date DESC, id DESC`、分頁回 items＋total。
  - `restore`：單一交易讀報廢列（不存在回 `Ok(None)`）→ INSERT 回 `assets`（id 用 `asset_id`、`created_at` 沿用、`updated_at` 為 `strftime('%Y-%m-%dT%H:%M:%SZ','now')`）→ DELETE 報廢列 → 提交後以 `assets::get` 回傳。
- `backend/src/assets.rs`：`escape_like` 提升為 `pub(crate)` 供 `list` 共用（doc 補「供資產與報廢清單搜尋共用」）。
- `backend/src/lib.rs` 加 `pub mod disposed_assets;`（`db` 與 `import` 之間，字母序）。
- 單元測試 9 支全綠：驗證（缺漏／空白、`2026/1/1`、`1-2-3`、日曆錯、明天、今天、過去）、dispose（全欄位搬移與欄位一致、`created_at` 相同、介面／指派／已歸還借還連動刪除；不存在回 None）、restore（原 id、`created_at` 保留、`updated_at` 更新、`tags` 還原、報廢列消失、無介面與指派；不存在回 None）、list（七欄搜尋、`%`／`_` 字面、分頁 total、`disposal_date` 新→舊＋同日期 id 決勝）。
- 驗收：`cargo test --manifest-path backend/Cargo.toml --lib disposed_assets` → 9 passed；`cargo fmt --manifest-path backend/Cargo.toml -- --check` → 通過；`cargo test --manifest-path backend/Cargo.toml --lib` → 195 passed（既有 186 全綠）。

偏離說明（無規格衝突）：

- `list` 回傳 `(Vec<DisposedAsset>, i64)`（items＋total），而非 `{items, total, page, per_page}` 結構——分頁包裝屬 API 層（票 02）職責，與 `assets::list`、`lendings::list_returned` 一致。
- `restore` 的 `id` 為報廢列自身 id（API 路徑 `/disposed-assets/{id}/restore`），復原入庫用 `asset_id`；與 spec §3 一致。
- `dispose` 以模組內私有 `AssetSnapshotRow` 讀取 `assets` 整列（`assets::AssetRow` 為模組私有無法重用），未新增公開 API。
- `DisposedAsset` 不含 `expired` 屆齡旗標（spec §3 未列；報廢列非在管資產）。
- 測試時間欄位以固定值入庫（`created_at`／`updated_at`），避免同秒內 `updated_at` 更新難以斷言（秒精度）。
