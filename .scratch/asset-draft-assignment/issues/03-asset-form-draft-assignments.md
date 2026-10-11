# 03: 資產對話框的指派草稿 UI

**What to build:** `AssetFormDialog.vue`：介面草稿列也能開啟指派（草稿模式），底部「已指派 IP」呈現草稿與已落地指派，草稿可取消，刪除介面列連動移除草稿（見 spec §4）。

**Blocked by:** 02

**Status:** done

- [x] 狀態：`assignmentDrafts: { key; rowKey; address; purpose; hostname; subnetId; subnetLabel }[]`、`subnetSummaries: SubnetSummary[] | null`（`prepare()` 載入，失敗為 `null`）。
- [x] 「指派 IP」按鈕一律顯示（移除 `row.id !== null` 條件）；`openAssign(row)`：已儲存列→現行 live 模式；草稿列→草稿模式（`draft` context：副標、介面名稱／MAC、該列既有指派與草稿、網段清單）。
- [x] `@draft` 事件：同位址（`parseAddress` 數值相等）＝更新該草稿，否則新增。
- [x] 底部「已指派 IP」：編輯模式一律顯示；新增模式於有草稿或已落地指派時顯示。草稿列＝位址、網段（已知時）、用途、hostname、「（未儲存）」＋「取消指派」（移除草稿、不跳確認、不提供「IP 管理」連結）；已落地列維持現行。
- [x] 刪除介面列：連動移除該列草稿；確認文字含「將一併取消 N 筆尚未儲存的指派」。
- [x] 草稿列的取消指派與介面刪除不觸發任何 API。
- [x] `pnpm typecheck`、`pnpm lint:check`、`pnpm build` 通過。

## Comments

實作完成（主實作 commit `1e5fca0`，`資產指派草稿：介面預填 eth0、草稿式指派與取消處置（前端）`）。

- `AssetFormDialog`：新增 `assignmentDrafts`、`subnetSummaries`（`prepare()` 載入；失敗為 `null`，略過 v6 預檢）、`assignDraftTarget`；`openAssign()` 依 `row.id` 分派 live／草稿；`draftAssignContext` 供草稿對話框（副標「財產編號(描述)（位置）（未儲存）」，描述未填顯示「新資產」、位置未填顯示「未填位置」）。
- `@draft` upsert：`sameAddress` 同位址＝更新（用途／hostname／網段），否則新增。
- 底部「已指派 IP」：新增模式於有草稿或已落地指派時顯示；草稿列標「（未儲存）」、可取消（不跳確認、無「IP 管理」連結）。
- 刪除介面列連動移除草稿，確認文字含「將一併取消 N 筆尚未儲存的指派」。
- `syncInterfaces()` 建立新介面後納入 `originalInterfaces` 並加入 `keptIds`（修正：部分失敗後該列已落地，指派入口原先會因找不到目標而無作用，且下次儲存可能誤刪）。
- 驗收：`pnpm typecheck`、`pnpm lint:check`、`pnpm build` 全綠。
