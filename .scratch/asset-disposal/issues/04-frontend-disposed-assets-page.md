# 04 — 前端：報廢資產頁與復原

Status: done
Blocked by: 02

## 目標

實作 `spec §6`：「報廢資產」頁（搜尋＋分頁＋復原）、路由與導覽項。

## 範圍

- 新增 `frontend/src/pages/DisposedAssetsPage.vue`：
  - 標題「報廢資產」；搜尋 `q-input`（placeholder 照 spec §6、300ms 去抖比照 `AssetsPage`）。
  - `q-table` 伺服器分頁（`[10, 25, 50]`、預設 10、`@request`）。
  - 欄位：財產編號、描述、位置、設備序號、廠牌、型號、購置日期、報廢日期、操作；空值「—」。
  - 「復原」`flat dense no-caps color="primary"`；`restoringId` 防連點；`$q.dialog` 二次確認（「復原資產」、說明介面與 IP 指派不會回復、描述以 `escapeHtml` 處理）→ `restoreDisposedAsset` → 通知「已復原資產」→ 重載。
- `frontend/src/router/routes.ts`：加 `disposed-assets`（`lendings` 之後）。
- `frontend/src/layouts/MainLayout.vue`：導覽項「報廢資產」（icon `archive`、`disposedAssetsActive`）置於「資產借還」之後、「清冊稽核」之前。

## 驗收

- `pnpm --filter frontend typecheck`
- `pnpm lint:check`
- `pnpm build:frontend`

## 注意

- 不要 `git commit`；不改後端；不動編輯對話框（票 03）。
- 復原後該列自清單消失（不顯示已復原紀錄，spec §2 不變量）。

## Comments

### 2026-10-12 完成紀錄（agent）

- 新增 `frontend/src/pages/DisposedAssetsPage.vue`：
  - 標題「報廢資產」；搜尋 `q-input`（placeholder 照 spec §6）以 `debounce` 300ms 去抖重載並回到第 1 頁（比照 `AssetsPage`）；另掛 `useCompositionGuard`（見偏離說明）。
  - `q-table` 伺服器分頁：`v-model:pagination`（預設 page 1／`rowsPerPage` 10）、`:rows-per-page-options="[10, 25, 50]"`、`@request` 更新 page／rowsPerPage 後重載，`rowsNumber` 取後端 `total`（比照 `LendingsPage` 已歸還清單）。
  - 欄位：財產編號、描述、位置、設備序號、廠牌、型號、購置日期、報廢日期、操作；空值（`property_no`／`device_serial`／`brand`／`model`／`purchase_date`）以 slot 顯示「—」（比照 `AssetsPage`）。未加排序（後端固定 `disposal_date DESC, id DESC`，API 無排序參數）。
  - 操作欄「復原」`flat dense no-caps color="primary"`；`restoringId` 控管同時間至多一筆（loading 防連點、其他列停用，比照借還頁「快速歸還」）→ `$q.dialog` 二次確認（標題「復原資產」、訊息含「介面與 IP 指派不會回復，需重新建立」、確定要復原「{description}」？、`html: true`、`cancel: true`、`persistent: true`、描述以本地 `escapeHtml` 處理，比照 `AssetsPage`）→ `restoreDisposedAsset(id)` → 通知「已復原資產」→ 重載清單（該列自清單消失）；失敗 negative 通知。
- `frontend/src/router/routes.ts`：`lendings` 之後新增 `{ path: "disposed-assets", component: () => import("@/pages/DisposedAssetsPage.vue") }`。
- `frontend/src/layouts/MainLayout.vue`：導覽項「報廢資產」（icon `archive`、`to="/disposed-assets"`、`disposedAssetsActive` computed）置於「資產借還」之後、「清冊稽核」之前。
- 驗收：`pnpm --filter frontend typecheck` → 通過；`pnpm lint:check` → 通過（先以 `pnpm --filter frontend lint` 跑 oxfmt 格式）；`pnpm build:frontend` → Build succeeded。

偏離說明：

- 搜尋欄另掛 `useCompositionGuard`＋`searchInputRef`（比照 `AssetsPage`／`IpListPage` 的搜尋欄位慣例，見 `.scratch/ime-composition/spec.md`）；票僅明寫 300ms 去抖，此為沿用既有搜尋欄位防注音組字異常的既有模式。
- 復原確認訊息首句寫為「復原後資產將回到資產清單；介面與 IP 指派不會回復，需重新建立。」（含票要求的必要字串）；票未逐字指定首句。
- 空值「—」僅套用於 DB 可為 NULL 的欄位；`description`／`location`／`disposal_date` 依 schema 為 NOT NULL（且描述／位置有非空白 CHECK），無空值情境。
- 未處理「復原後最後一頁變空」的分頁回退（比照 `AssetsPage` 刪除後的行為）；票未要求。
