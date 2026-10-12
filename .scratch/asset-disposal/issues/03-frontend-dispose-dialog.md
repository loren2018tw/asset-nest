# 03 — 前端：編輯對話框「報廢」流程

Status: done
Blocked by: 02

## 目標

實作 `spec §5`：報廢 API client、紅色「報廢」按鈕與確認子對話框。

## 範圍

- 新增 `frontend/src/api/disposedAssets.ts`：`DisposedAsset`、`DisposeInput`、`DisposeResponse`（`kea_sync: KeaSyncState[]`，由 `@/api/kea` 匯入）、`DisposedAssetPage`、`DisposedAssetListParams`；`disposeAsset`、`listDisposedAssets`、`restoreDisposedAsset`；query string 比照 `api/assets.ts` 的 `queryString`。
- 新增 `frontend/src/utils/localDate.ts`：`localDateString()`（本地時區 `YYYY-MM-DD`，勿用 `toISOString()`）。
- `frontend/src/components/AssetFormDialog.vue`：
  - `asset` prop 型別擴充 `(Asset & { lending?: LendingBrief | null }) | null`；`AssetsPage` 的 `editing` 型別同步調整（若需要）。
  - 按鈕列改 `class="q-pa-md"`：僅編輯模式顯示左側 `flat color="negative"`「報廢」鈕；`<q-space />`；既有「取消」「儲存」。出借中停用＋tooltip「出借中，請先歸還」（`<span>` 包法比照 `AssetsPage` 刪除鈕）。
  - 新增子對話框（比照 `cancelConfirmOpen`）：標題「報廢資產」；影響摘要（介面數／指派數／其中保留數／最後可見／介面 MAC，比照 `AssetsPage.confirmDeleteWithImpact`）；`type="date"` 報廢日期（預設今天、`:max="today"`、必填 rule）；提示「報廢後可於『報廢資產』頁復原；介面與 IP 指派不會回復。對話框中未儲存的變更將被捨棄。」；按鈕「取消」／「確認報廢」（`flat color="negative"`、`:loading="disposing"`）。
  - 確認成功：關閉子對話框與主對話框、`emit("saved")`、通知「已報廢資產」；`kea_sync` 任一 `failed` → warning 提示一次。失敗：negative 通知、留在對話框。
  - `prepare()` 重置子對話框狀態。

## 驗收

- `pnpm --filter frontend typecheck`
- `pnpm lint:check`
- `pnpm build:frontend`（SFC 編譯驗證）

## 注意

- 出借摘要僅資產清單入口帶入；IP 清單的綁定資產編輯入口無摘要時不預先停用，後端 409 把關（spec §5 已知限制）。
- 不要 `git commit`；不改後端；不動報廢頁（票 04）。
- 樣式沿用 Quasar 既有慣例（`flat color="negative"`）。

## Comments

### 2026-10-12 完成紀錄（agent）

- 新增 `frontend/src/api/disposedAssets.ts`：`DisposedAsset`（對映後端欄位；`tags: string[]`）、`DisposeInput`、`DisposeResponse`（`extends DisposedAsset`＋`kea_sync: KeaSyncState[]`，`KeaSyncState` 由 `@/api/kea` 匯入）、`DisposedAssetListParams`、`DisposedAssetPage`；`disposeAsset`（POST `/api/v1/assets/{id}/disposal`）、`listDisposedAssets`（GET `/api/v1/disposed-assets`，query string 比照 `api/assets.ts` 的 `queryString`）、`restoreDisposedAsset`（POST `/api/v1/disposed-assets/{id}/restore`，body `{}`）。
- 新增 `frontend/src/utils/localDate.ts`：`localDateString()` 以本地時區組 `YYYY-MM-DD`（`getFullYear`／`getMonth`／`getDate`，未用 `toISOString()`）。
- `frontend/src/components/AssetFormDialog.vue`：
  - `asset` prop 型別擴充 `(Asset & { lending?: LendingBrief | null }) | null`（`LendingBrief` 由 `@/api/lendings` 匯入）；`AssetsPage.vue` 新增區域型別 `EditingAsset = Asset & { lending?: LendingBrief | null }`，套用於 `editing`／`openEdit`。
  - 按鈕列改 `class="q-pa-md"`：僅編輯模式左側 `flat color="negative"`「報廢」鈕；出借中（`lending` 有值）停用＋`<span>` 包法 tooltip「出借中，請先歸還」（比照 `AssetsPage` 刪除鈕）；`<q-space />`＋既有「取消」「儲存」。
  - 新增 `persistent` 子對話框（比照 `cancelConfirmOpen`）：標題「報廢資產」；影響摘要（介面數＝`originalInterfaces.length`、指派數＝`assignments.length`、其中保留數、最後可見＝`lastSeenAt` 相對時間、介面 MAC，比照 `AssetsPage.confirmDeleteWithImpact`）；`type="date"` 報廢日期（預設今天、`:max="today"`、必填 rule）；後果與未儲存變更提示（照票文字）；按鈕「取消」／「確認報廢」（`flat color="negative"`、`:loading="disposing"`）。
  - 確認成功：`disposeAsset` → `notifyKeaSync`（`kea_sync.find(status === "failed")`，多筆僅提示一次、訊息比照 `utils/keaSync.ts`）→ 通知「已報廢資產」→ 關閉子對話框與主對話框 → `emit("saved")`；失敗 negative 通知、留在對話框。
  - `prepare()` 重置 `disposalOpen`／`disposalDate`（今天）／`disposing`；`openDisposal()` 開啟時同步更新 `today`（`:max`）與預設日期，跨日不殘留舊上限。
- 驗收：`pnpm --filter frontend typecheck` → 通過；`pnpm lint:check` → 通過（先以 `pnpm --filter frontend lint` 修正 oxfmt 格式）；`pnpm build:frontend` → Build succeeded。

偏離說明：

- 子對話框報廢日期欄加 `outlined dense`（與全站表單欄位慣例一致）；票未指定樣式。
- 子對話框「取消」鈕加 `:disable="disposing"`（比照 `cancelConfirmOpen` 子對話框「忙碌中停用」慣例），防送出中關閉；票僅明列確認鈕 `:loading`。
- 必填 rule 於失焦顯示（`lazy-rules`），確認前未另以 `q-form` 擋送；清空日期送出由後端 400（「報廢日期為必填」）把關，negative 通知後留在對話框（符合票的失敗行為）。
- `AssetsPage.vue` 以區域型別別名實作票的型別擴充，未新增共用型別或動其他頁面（`IpListPage`／`KeaLeasesPage` 傳入的純 `Asset` 仍相容）。
