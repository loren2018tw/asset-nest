# spec — 資產報廢（asset disposal）

- 狀態：已定案（經 2026-10-12 grilling 兩輪逐題確認；Q7「支援復原」、Q8「報廢資產頁＋復原鈕」依使用者決定，其餘採建議）
- 詞彙：`GLOSSARY.md`「報廢詞彙」（報廢、報廢資產、報廢日期、復原）已登錄；決策見 `docs/adr/0026`

## 1. 背景與目標

資產目前只有 CRUD、IP 指派與借還，無「報廢」概念（`inventory-audit` spec 曾明確列為非目標）。實務上設備損壞或逾齡汰換後需移出在管資產，但資料要留存供財管查核。本功能：

1. 新增「報廢資產資料表」（`disposed_assets`）：報廢＝將資產整列複製進該表（＋報廢日期）後，自 `assets` 刪除。
2. 資產編輯對話框左下角新增紅色「報廢」按鈕：填入報廢日期（預設今天）後確認報廢；介面與 IP 指派一併釋出（含 Kea 保留推送刪除）。
3. 新增「報廢資產」頁（`/disposed-assets`）：清單＋關鍵字搜尋＋伺服器分頁，每列可「復原」。
4. 復原＝將欄位搬回 `assets`（沿用原 id 與 `created_at`）並刪除報廢紀錄；介面與 IP 指派**不**回復。

非目標：報廢歷程保留（復原後不留紀錄）、批次報廢、報廢清冊匯出、已刪除資產不推送 Kea 的既有缺口補救（另案）。

## 2. 名詞與不變量

名詞見 `GLOSSARY.md`「報廢詞彙」。

不變量：

- 出借中的 Asset 不可報廢 → 409「此資產出借中，請先歸還」（比照刪除）。
- 報廢日期必填、`YYYY-MM-DD`、不可為未來（今天可）、可回填過去。
- 報廢時：介面、指派（含保留）與借還紀錄（含已歸還歷史）連動刪除（FK CASCADE）；受管網段的「保留」推送 Kea 刪除，失敗不阻擋本地儲存（ADR-0011），僅回警示。
- 報廢不保留 MAC 快照；觀測資料以 MAC／網段為鍵，不受影響。
- 同一資產同時至多一筆報廢紀錄（`disposed_assets.asset_id` UNIQUE；復原即刪除該列）。
- 復原：資產以原 id、原 `created_at` 回到 `assets`，`updated_at` 更新為當下；介面與指派不回復；報廢列刪除。
- 報廢與復原皆不記操作人、不設備註、不記復原時間。
- 報廢後資產不出現在資產清單、搜尋、匯出、清冊稽核「系統有」側與指派入口；復原後重新出現。

## 3. 資料模型

新增 `backend/migrations/0012_disposed_assets.sql`（欄位為 `assets` 的複製＋`asset_id`＋`disposal_date`；`created_at`／`updated_at` 複製原值）：

```sql
-- 票 01：資產報廢。disposed_assets 資料表（見 spec §3、ADR-0026）。
-- 報廢＝整列複製到此表＋報廢日期後自 assets 刪除；復原＝搬回後刪除此列。
CREATE TABLE IF NOT EXISTS disposed_assets (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    asset_id       INTEGER NOT NULL UNIQUE,
    property_no    TEXT,
    description    TEXT NOT NULL CHECK (trim(description) <> ''),
    location       TEXT NOT NULL CHECK (trim(location) <> ''),
    device_serial  TEXT,
    brand          TEXT,
    model          TEXT,
    purchase_date  TEXT,
    lifespan_years INTEGER CHECK (lifespan_years IS NULL OR lifespan_years >= 0),
    note           TEXT,
    tags           TEXT NOT NULL DEFAULT '[]',
    disposal_date  TEXT NOT NULL,
    created_at     TEXT NOT NULL,
    updated_at     TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_disposed_assets_disposal_date ON disposed_assets(disposal_date);
```

新模組 `backend/src/disposed_assets.rs`（比照 `assets.rs`／`lendings.rs` 風格）：

- `const COLUMNS`：`id, asset_id, property_no, description, location, device_serial, brand, model, purchase_date, lifespan_years, note, tags, disposal_date, created_at, updated_at`。
- 型別：`DisposedAssetRow`（FromRow，`tags: String`）、`pub struct DisposedAsset`（Serialize；`tags: Vec<String>`）、`pub struct DisposeInput { pub disposal_date: Option<String> }`、`pub struct ValidDispose { disposal_date: String }`。
- 驗證 `DisposeInput::validate(self, today: NaiveDate) -> Result<ValidDispose, ApiError>`：
  - 必填（field `disposal_date`，訊息「報廢日期為必填」）。
  - 嚴格 `YYYY-MM-DD`（4-2-2 位數字＋`NaiveDate` 日曆檢查，比照 `lendings::validate_due_date`；訊息「報廢日期須為有效日期（YYYY-MM-DD）」）。
  - 不可為未來：`disposal_date > today` → 訊息「報廢日期不可為未來日期」（field `disposal_date`）。
- 查詢：
  - `dispose(pool, asset_id, valid) -> Result<Option<DisposedAsset>, sqlx::Error>`：單一交易——讀 `assets` 整列（不存在回 `Ok(None)`，`tags` 保持 JSON 字串）→ `INSERT INTO disposed_assets`（複製全部欄位＋`asset_id`＋`disposal_date`）→ `DELETE FROM assets WHERE id = ?`（CASCADE 清介面／指派／借還）→ 回新列。
  - `list(pool, q: Option<&str>, page: i64, per_page: i64) -> Result<(Vec<DisposedAsset>, i64), sqlx::Error>`：`q` 以 LIKE 子字串（轉義 `%`／`_`，比照 `assets::escape_like`，將其提升為 `pub(crate)` 共用）比對 `property_no／description／location／device_serial／brand／model／note`；排序 `disposal_date DESC, id DESC`；回 items 與 total。
  - `restore(pool, id) -> Result<Option<Asset>, sqlx::Error>`：單一交易——讀報廢列（不存在回 `Ok(None)`）→ `INSERT INTO assets (id, …, created_at, updated_at)`（id 用 `asset_id`、`created_at` 沿用、`updated_at` 為 `strftime('%Y-%m-%dT%H:%M:%SZ','now')`）→ `DELETE` 報廢列 → 提交後以 `assets::get` 回傳。
- `backend/src/lib.rs` 加 `pub mod disposed_assets;`（`db` 與 `import` 之間，字母序）。
- 單元測試（`#[cfg(test)]`，`:memory:`＋migrations）：
  - 驗證：缺漏／空白 → 必填；`2026/1/1`、`1-2-3` → 格式錯；明天 → 不可未來；今天、過去 → 過。
  - dispose：建立含全欄位（含 `tags`、`property_no`、`device_serial`）資產＋介面＋指派＋已歸還借還 → 搬移後 `assets` 無、報廢列欄位與原值一致（`created_at` 相同、`disposal_date` 正確）、介面／指派／借還列消失。
  - restore：原 id、`created_at` 保留、`updated_at` 更新、`tags` 還原、報廢列消失、資產無介面與指派。
  - `list`：搜尋命中欄位、`%` 視為字面、分頁 total 與排序（`disposal_date` 新→舊）。

## 4. API

新模組 `backend/src/api/disposed_assets.rs`，於 `api/mod.rs` 的 v1 路由 `.merge`（`auth` 與 `import` 之間）：

| 方法 | 路徑 | 行為 |
|------|------|------|
| POST | `/api/v1/assets/{id}/disposal` | body `{ disposal_date }`。400 驗證；404 資產不存在；409 出借中。201＋`DisposedAsset` 欄位攤平＋`kea_sync: Vec<KeaSync>`。 |
| GET | `/api/v1/disposed-assets?q=&page=&per_page=` | `{ items, total, page, per_page }`；`page` 由 1、`per_page` 預設 10 夾 1..=100（比照 `lendings` 已歸還清單）。 |
| POST | `/api/v1/disposed-assets/{id}/restore` | 200＋`Asset`；404「找不到報廢資產」。 |

`POST /assets/{id}/disposal` 順序：

1. 驗證（`DisposeInput::validate(Local::now().date_naive())`）。
2. `assets::get` 不存在 → 404「找不到資產」。
3. `lendings::has_open_lending` → 409「此資產出借中，請先歸還」。
4. Kea 推送：對 `assignments::list_for_asset` 中 `purpose == "reservation"` 者，逐筆（`subnet_id` 快取）以 `subnets::get` 取網段後呼叫 `kea::sync::after_assignment_change(&state.db, state.kea.as_ref(), &subnet, Some(&assignment), None)`；收集 `Some(sync)` 成 `kea_sync`。未受管網段或未設 client 自然回 `None`；推送失敗為 `KeaSync::failed`，不阻擋（ADR-0011）。
5. `disposed_assets::dispose` → 201。

回應型別：`DisposeResponse { #[serde(flatten)] disposed: DisposedAsset, kea_sync: Vec<KeaSync> }`。

錯誤：`ApiError::validation/not_found/conflict/internal`（見 `api/error.rs`）。

整合測試 `backend/tests/disposal.rs`（比照 `delete_linkage.rs` 的 `test_pool`／`send` helpers）：

- 流程：建立資產（全欄位）＋介面＋指派（static）→ 報廢 201 → `GET /assets/{id}` 404、資產清單不含 → `GET /disposed-assets` 含該筆（欄位與 `disposal_date`）→ 原 IP 可重新指派給另一資產 → 復原 200（原 id、無介面／指派）→ 報廢清單不含、資產清單含。
- 驗證：缺日期／`2026/1/1`／未來日期 → 400（field `disposal_date`）；今天可。
- 出借中 → 409；歸還後可報廢；報廢連動刪除借還紀錄。
- 復原：不存在／重複復原 → 404。
- 清單：`q` 搜尋、`page`／`per_page`、`per_page` 夾 1..=100。
- Kea（比照 `tests/kea_sync.rs` 的 stub 手法）：受管網段＋保留 → 報廢後 stub 保留消失、回應 `kea_sync` 為 `ok`；stub 失敗時仍 201、`kea_sync` 為 `failed`、本地已搬移。

## 5. 前端：編輯對話框報廢

新模組 `frontend/src/api/disposedAssets.ts`：

- `interface DisposedAsset`（對映後端欄位；`tags: string[]`）、`interface DisposeInput { disposal_date: string }`、`interface DisposeResponse extends DisposedAsset { kea_sync: KeaSyncState[] }`（`KeaSyncState` 由 `@/api/kea` 匯入）、`DisposedAssetPage { items, total, page, per_page }`、`DisposedAssetListParams { q?, page?, per_page? }`。
- `disposeAsset(assetId, input): Promise<DisposeResponse>`（`POST /api/v1/assets/{id}/disposal`）、`listDisposedAssets(params): Promise<DisposedAssetPage>`、`restoreDisposedAsset(id): Promise<Asset>`（`POST /api/v1/disposed-assets/{id}/restore`，body `{}`）。
- query string 比照 `api/assets.ts` 的 `queryString` 寫法。

新增 `frontend/src/utils/localDate.ts`：`localDateString(): string` 以本地時區組 `YYYY-MM-DD`（勿用 `toISOString()`，UTC 會跨日）。

`AssetFormDialog.vue`：

- `asset` prop 型別擴充為 `(Asset & { lending?: LendingBrief | null }) | null`（資產清單列帶得出借摘要；IP 清單的綁定資產編輯入口無摘要時不預先停用，後端 409 把關）。
- 按鈕列由 `align="right"` 改為 `class="q-pa-md"`：左側（僅編輯模式，`props.asset !== null`）`flat color="negative"` 的「報廢」鈕；`<q-space />`；既有「取消」「儲存」。出借中（`lending` 有值）時停用＋tooltip「出借中，請先歸還」（比照清單刪除鈕的 `<span>` 包法）。
- 新增子對話框（比照 `cancelConfirmOpen` 模式，`persistent`）：標題「報廢資產」；內容：
  - 影響摘要（比照 `AssetsPage.confirmDeleteWithImpact`）：介面數（`originalInterfaces.length`）、指派數（`assignments.length`）、其中保留數、最後可見（`lastSeenAt`，相對時間）、介面 MAC（`—` 表無）。
  - `<q-input type="date" label="報廢日期" :rules="[必填]" :max="today">`，預設今天。
  - 提示文字：「報廢後可於『報廢資產』頁復原；介面與 IP 指派不會回復。對話框中未儲存的變更將被捨棄。」
  - 按鈕：「取消」／「確認報廢」（`flat color="negative"`，`:loading="disposing"` 防連點）。
- 確認：`disposeAsset(asset.id, { disposal_date })` → 成功：關閉子對話框與主對話框、`emit("saved")`、通知「已報廢資產」；`kea_sync` 任一 `status === "failed"` 時另以 warning 提示一次（訊息比照 `utils/keaSync.ts`）。失敗：negative 通知，留在對話框。

## 6. 前端：報廢資產頁

新增 `frontend/src/pages/DisposedAssetsPage.vue`：

- 標題「報廢資產」；搜尋 `q-input`（placeholder「搜尋財產編號／描述／位置／設備序號／廠牌／型號／備註」，300ms 去抖，比照 `AssetsPage` 的 `debounce`）。
- `q-table` 伺服器分頁：`rows-per-page-options [10, 25, 50]`、預設 10、`@request` 重載。
- 欄位：財產編號、描述、位置、設備序號、廠牌、型號、購置日期、報廢日期、操作（空值顯示「—」）。
- 操作欄「復原」：`flat dense no-caps color="primary"`；`restoringId` 控管（同時間至多一筆、loading 防連點，比照借還頁「快速歸還」）。
- 點擊復原 → `$q.dialog` 二次確認（標題「復原資產」；訊息含「介面與 IP 指派不會回復，需重新建立」；確定要復原「{description}」？；`html: true`、`cancel: true`、`persistent: true`，描述以 `escapeHtml` 處理，比照 `AssetsPage`）→ `restoreDisposedAsset(id)` → 通知「已復原資產」→ 重新載入清單。

路由與導覽：

- `frontend/src/router/routes.ts`：`{ path: "disposed-assets", component: () => import("@/pages/DisposedAssetsPage.vue") }`（`lendings` 之後）。
- `frontend/src/layouts/MainLayout.vue`：導覽項「報廢資產」（icon `archive`、`to="/disposed-assets"`、`disposedAssetsActive` computed）置於「資產借還」之後、「清冊稽核」之前。

## 7. 連動與非目標

- 資產清單、搜尋、匯出、清冊稽核、指派入口自動不含報廢資產；復原後自動恢復（無需改動既有功能）。
- 清冊仍列已報廢財產編號時，清冊稽核顯示「清冊有、系統沒有」屬實（接受）。
- 非目標：報廢歷程（復原後不留紀錄）、批次報廢、報廢清冊匯出、`DELETE /assets/{id}` 的 Kea 推送補救（既有缺口，另案）。

## 8. 測試

- 後端：模組單元測試（§3）＋整合測試 `backend/tests/disposal.rs`（§4）。
- 前端：無測試框架；以 `pnpm --filter frontend typecheck`、`pnpm lint:check`、`pnpm build:frontend` 驗證；行為由驗證票（05）以端到端煙霧測試涵蓋。
