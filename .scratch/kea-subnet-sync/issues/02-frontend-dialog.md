# 02: 前端 Kea 同步對話框顯示網段層差異

**What to build:** `api/kea.ts` 型別擴充（`KeaPlanSubnet` 的 `pool_add`／`pool_delete`／`gateway`、totals、`KeaApplySubnet` 的 `pool_added`／`pool_deleted`／`gateway_updated`／`settings_error`）；`KeaSyncDialog.vue` 計畫與結果呈現位址池與 gateway 差異、明細與錯誤；僅有網段層差異時「套用同步」仍可按。詳見 `.scratch/kea-subnet-sync/spec.md`。

**Blocked by:** 01（型別與回應欄位）

**Status:** ready-for-agent

- [ ] 型別擴充
- [ ] 計畫 banner／每網段摘要含位址池與 gateway 計數；明細列出 pool 新增／刪除與 gateway `current → desired`（`null` 文案）
- [ ] `canApply` 涵蓋僅網段層差異
- [ ] 結果 banner／每網段列含計數；`settings_error` negative 顯示並使 notify 為 warning
- [ ] `pnpm lint:check`、`pnpm --filter frontend typecheck` 綠；票檔 Comments＋commit（不 push）

## Comments
