# 02: 系統狀態頁「清除觀測紀錄」按鈕與文件

**What to build:** 「觀測代理」卡片標題列右側新增「清除觀測紀錄」按鈕（negative outline、`delete_sweep`、tooltip、執行中 loading），二次確認（`$q.dialog`）後呼叫 `DELETE /api/v1/observations`；成功以 `$q.notify` 顯示刪除筆數並重載頁面，失敗負向通知。README 與 GLOSSARY 補文件。

**Blocked by:** 01

**Status:** done

- [x] `api/observations.ts`：`ClearObservationsResult` 型別＋`clearObservations()`（`apiDelete`）
- [x] `KeaStatusPage.vue`：按鈕＋確認框（persistent、可取消、確定鈕「清除」negative）＋成功／失敗通知＋重載
- [x] `GLOSSARY.md`「觀測詞彙」新增「清除觀測紀錄」；`README.md`「新鮮度與保留」補手動清除說明
- [x] `pnpm lint:check`、`pnpm --filter frontend typecheck` 綠
- [ ] 瀏覽器手動 QA（確認框文案、通知筆數、重載後代理表不受影響、IP 清單與網段外觀測反映清除）——待使用者確認

## Comments

實作完成（未 commit）。

- `frontend/src/api/observations.ts`：新增 `ClearObservationsResult` 與 `clearObservations()`（`apiDelete<…>("/api/v1/observations")`）。
- `frontend/src/pages/KeaStatusPage.vue`：「觀測代理」卡片標題列右側新增「清除觀測紀錄」按鈕（negative outline、`delete_sweep`、tooltip、`:loading="clearing"`）；`$q.dialog` 二次確認（標題／內文照 spec、確定鈕「清除」negative、persistent、可取消）；成功 `$q.notify`「已清除觀測紀錄（現況 N 筆、事件 M 筆）」並 `load()` 重載；失敗 negative 通知。
- `GLOSSARY.md`：「觀測詞彙」新增「清除觀測紀錄」；`README.md`：「新鮮度與保留」補手動清除說明。
- 驗收：`pnpm lint:check`（oxfmt＋oxlint）、`pnpm typecheck` 全綠；後端資料效果以真機 smoke 驗證（見票 01）；瀏覽器互動（確認框、通知、重載）待使用者確認。
