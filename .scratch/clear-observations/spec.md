# 規格：清除觀測紀錄（手動清除全部觀測資料）

- 狀態：ready-for-agent（2026-10-13）
- 詞彙依 `GLOSSARY.md`「觀測詞彙」（新增「清除觀測紀錄」）；決策沿用 `docs/adr/0014`（觀測與宣告分離：觀測資料可被清除而不影響宣告）與 `docs/adr/0016`（觀測記錄模型）。
- 範圍：全站一次清除；不含逐網段清除、被拒回報清除與檔案縮減。

## Problem Statement

觀測資料（`ip_presence` 現況與 `observation_event` 事件）目前只有保留期自動清理，而且只刪事件：現況列沒有任何清除途徑。當觀測資料被污染（代理接錯網段、網段改動前的舊資料、測試殘留），管理者無法讓觀測層回到乾淨狀態；網段外觀測的殘留列與觀測衍生衝突標記也只能一直掛著。

## Solution

系統狀態頁「觀測代理」區塊新增「清除觀測紀錄」按鈕：二次確認後以單一交易刪除全部觀測現況與變化事件（含網段外觀測列），並回報刪除筆數。宣告（指派／保留）、代理登錄與被拒回報完全不受影響；在線代理下一輪回報後資料自然重新累積。

## User Stories

1. 作為管理者，我想一鍵清除全部觀測紀錄（現況與事件），以便在資料污染或環境改動後讓觀測層重新開始。
2. 作為管理者，我想在清除前看到明確的二次確認（含「宣告不受影響、代理會重新累積」說明），以免誤按。
3. 作為管理者，我想在清除後看到刪除筆數，以便確認動作生效。
4. 作為管理者，我想清除只動觀測層——指派、保留、資產、代理登錄與被拒回報完全不變。

## Implementation Decisions

### 後端

- `observation` 模組新增 `ClearCounts { presence_deleted, events_deleted }` 與 `clear_observations(pool)`：單一交易刪除 `observation_event` 與 `ip_presence`（含 `out_of_subnet` 列），以 `rows_affected` 計數；冪等；不動 `agent`、`agent_auth_failure` 與宣告資料；不 `VACUUM`（頁面重用，檔案不縮）。
- `api/observations.rs` 新增 `DELETE /api/v1/observations` → 200 `{"presence_deleted":N,"events_deleted":M}`；非白名單端點，需登入 session；完成記 `tracing::info`。
- 無 migration（schema 不變）。

### 前端

- `api/observations.ts` 新增 `clearObservations()`（`apiDelete`，回 `ClearObservationsResult`）。
- `KeaStatusPage.vue`「觀測代理」卡片標題列右側新增按鈕：`outline`、`color="negative"`、icon `delete_sweep`、label「清除觀測紀錄」、tooltip「刪除全部觀測現況與事件；宣告（指派／保留）不受影響」；執行中 `loading`。
- 確認框（`$q.dialog`，`persistent`、`cancel: true`）：標題「清除觀測紀錄」、內文「將刪除全部觀測現況與事件（含網段外觀測），此動作無法復原。指派與保留等宣告資料不受影響；在線代理會在下一輪回報後重新累積資料。確定要清除？」、確定鈕「清除」（negative）。
- 成功 `$q.notify`「已清除觀測紀錄（現況 N 筆、事件 M 筆）」＋重載頁面；失敗 negative 通知。

### 文件

- `GLOSSARY.md`「觀測詞彙」新增「清除觀測紀錄」。
- `README.md`「新鮮度與保留」補手動清除說明。

## Testing Decisions

- 原則沿用：只測外部行為；記憶體 SQLite＋`sqlx::migrate!`＋`tower::ServiceExt::oneshot`。
- 新增 `backend/tests/observations_clear.rs`：
  - 種子（網段、資產、介面、指派、代理、被拒回報、現況含網段外列、事件）→ `DELETE /api/v1/observations` → 200 筆數正確；兩表清空；宣告／代理／被拒回報筆數不變。
  - 再次呼叫 → 200 零筆（冪等）。
  - 啟用登入認證的 `AppState`：未帶 session cookie → 401；帶有效 token → 200（比照 `tests/auth.rs`）。
- 前端維持 `pnpm lint:check`、`pnpm --filter frontend typecheck` 與手動 QA（確認框、通知、重載、其他頁面反映）。

## Out of Scope

- 逐網段／逐代理清除；清除被拒回報；檔案縮減（`VACUUM`）；審計紀錄 UI；自動清除排程變更；前端自動化測試。

## Further Notes

- 清除後空窗期：已指派位址在涵蓋仍在時顯示「從未上線」，代理回報後恢復；觀測衍生衝突標記（`ObservedMacMismatch`、`ObservedOnUnassigned`）與網段外觀測清單同步歸零並重新累積。
- 與既有「代理失聯時資料保留、不清除」不衝突：那是自動行為承諾，本功能是管理者主動清除。

## 建議票切

(1) 後端端點＋測試；(2) 前端按鈕＋文件。
