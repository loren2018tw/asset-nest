# 05: 取消新增時已建立資產的三選項處置

**What to build:** `AssetFormDialog.vue`「取消」按鈕：未曾建立資產時直接關閉；已建立資產且尚未完成儲存時，跳三選項對話框（繼續編輯／保留已建立內容／刪除已建立內容；見 spec §5.3）。

**Blocked by:** 04

**Status:** ready-for-agent

- [ ] 「取消」改為 `@click`（移除 `v-close-popup`）：`savedAssetId === null` → 直接關閉；否則開啟確認對話框。
- [ ] 確認對話框（`q-dialog` 自繪，Quasar 2.34 的 `$q.dialog` 僅支援 ok/cancel 兩顆）：說明此新資產已建立、尚未完成儲存。
- [ ] 「保留已建立內容」→ 關閉對話框（資料保留）。
- [ ] 「刪除已建立內容」→ `DELETE /assets/{id}`（連動刪除介面與已落地指派）、`emit("saved")` 刷新外層清單、通知「已刪除」；失敗 → 回到表單顯示錯誤。
- [ ] `pnpm typecheck`、`pnpm lint:check`、`pnpm build` 通過。

## Comments
