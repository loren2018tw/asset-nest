# 02: 前端 Kea 同步對話框顯示網段層差異

**What to build:** `api/kea.ts` 型別擴充（`KeaPlanSubnet` 的 `pool_add`／`pool_delete`／`gateway`、totals、`KeaApplySubnet` 的 `pool_added`／`pool_deleted`／`gateway_updated`／`settings_error`）；`KeaSyncDialog.vue` 計畫與結果呈現位址池與 gateway 差異、明細與錯誤；僅有網段層差異時「套用同步」仍可按。詳見 `.scratch/kea-subnet-sync/spec.md`。

**Blocked by:** 01（型別與回應欄位）

**Status:** done

- [x] 型別擴充
- [x] 計畫 banner／每網段摘要含位址池與 gateway 計數；明細列出 pool 新增／刪除與 gateway `current → desired`（`null` 文案）
- [x] `canApply` 涵蓋僅網段層差異
- [x] 結果 banner／每網段列含計數；`settings_error` negative 顯示並使 notify 為 warning
- [x] `pnpm lint:check`、`pnpm --filter frontend typecheck` 綠；票檔 Comments＋commit（不 push）

## Comments

實作完成（主實作 commit `169fc63`，`02 Kea 網段層同步：前端對話框顯示 pool／gateway 差異`）。

- `frontend/src/api/kea.ts`：新增 `KeaGatewayPlan`；`KeaPlanSubnet` 增 `pool_add`／`pool_delete`／`gateway`；`KeaSyncPlan.totals` 增 `pool_add`／`pool_delete`／`gateway`；`KeaApplySubnet` 增 `pool_added`／`pool_deleted`／`gateway_updated`／`settings_error`。
- `KeaSyncDialog.vue`：計畫 banner 與每網段摘要含位址池新增／刪除與 gateway 變更；明細新增「新增／刪除位址池」與 gateway `current → desired`（`null` 以「（未設）」／「（移除）」呈現）；`canApply` 涵蓋僅網段層差異；結果 banner／每網段列含計數、`settings_error` 以 negative 顯示、`$q.notify` 視為部分失敗；`hasDetail` 讓純網段層差異也可展開明細。
- 驗收：`pnpm --filter frontend typecheck` 綠；`pnpm lint:check` 綠（oxfmt＋oxlint）；`pnpm build:frontend` 成功。
