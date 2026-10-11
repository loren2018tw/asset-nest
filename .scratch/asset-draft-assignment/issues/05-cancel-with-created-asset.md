# 05: 取消新增時已建立資產的三選項處置

**What to build:** `AssetFormDialog.vue`「取消」按鈕：未曾建立資產時直接關閉；已建立資產且尚未完成儲存時，跳三選項對話框（繼續編輯／保留已建立內容／刪除已建立內容；見 spec §5.3）。

**Blocked by:** 04

**Status:** done

- [x] 「取消」改為 `@click`（移除 `v-close-popup`）：`savedAssetId === null` → 直接關閉；否則開啟確認對話框。
- [x] 確認對話框（`q-dialog` 自繪，Quasar 2.34 的 `$q.dialog` 僅支援 ok/cancel 兩顆）：說明此新資產已建立、尚未完成儲存。
- [x] 「保留已建立內容」→ 關閉對話框（資料保留）。
- [x] 「刪除已建立內容」→ `DELETE /assets/{id}`（連動刪除介面與已落地指派）、`emit("saved")` 刷新外層清單、通知「已刪除」；失敗 → 回到表單顯示錯誤。
- [x] `pnpm typecheck`、`pnpm lint:check`、`pnpm build` 通過。

## Comments

實作完成（主實作 commit `1e5fca0`，`資產指派草稿：介面預填 eth0、草稿式指派與取消處置（前端）`）。

- 「取消」改 `@click="requestCancel"`（`saving` 時停用，避免儲存進行中開啟處置對話框）：`savedAssetId === null` 直接關閉；否則開三選項 `q-dialog`（此版 Quasar 的 `$q.dialog` 僅支援 ok／cancel 兩顆，故自繪；`deletingAsset` 時另兩顆停用）。
- 「繼續編輯」留在表單；「保留已建立內容」關閉並 `emit("saved")`（部分落地後清單可能過期）；「刪除已建立內容」`deleteAsset`（連動刪除介面與指派）＋`emit("saved")`＋通知，失敗留在表單顯示錯誤。
- 驗收：`pnpm typecheck`、`pnpm lint:check`、`pnpm build` 全綠。
