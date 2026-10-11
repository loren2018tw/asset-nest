# 03: 資產對話框的指派草稿 UI

**What to build:** `AssetFormDialog.vue`：介面草稿列也能開啟指派（草稿模式），底部「已指派 IP」呈現草稿與已落地指派，草稿可取消，刪除介面列連動移除草稿（見 spec §4）。

**Blocked by:** 02

**Status:** ready-for-agent

- [ ] 狀態：`assignmentDrafts: { key; rowKey; address; purpose; hostname; subnetId; subnetLabel }[]`、`subnetSummaries: SubnetSummary[] | null`（`prepare()` 載入，失敗為 `null`）。
- [ ] 「指派 IP」按鈕一律顯示（移除 `row.id !== null` 條件）；`openAssign(row)`：已儲存列→現行 live 模式；草稿列→草稿模式（`draft` context：副標、介面名稱／MAC、該列既有指派與草稿、網段清單）。
- [ ] `@draft` 事件：同位址（`parseAddress` 數值相等）＝更新該草稿，否則新增。
- [ ] 底部「已指派 IP」：編輯模式一律顯示；新增模式於有草稿或已落地指派時顯示。草稿列＝位址、網段（已知時）、用途、hostname、「（未儲存）」＋「取消指派」（移除草稿、不跳確認、不提供「IP 管理」連結）；已落地列維持現行。
- [ ] 刪除介面列：連動移除該列草稿；確認文字含「將一併取消 N 筆尚未儲存的指派」。
- [ ] 草稿列的取消指派與介面刪除不觸發任何 API。
- [ ] `pnpm typecheck`、`pnpm lint:check`、`pnpm build` 通過。

## Comments
