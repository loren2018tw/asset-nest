# 01: 新增介面時清單為空預填 eth0

**What to build:** `AssetFormDialog.vue` 的「新增介面」按鈕：按下時若當下介面清單為空，新列名稱預填 `eth0`（MAC、備註空白）。新增與編輯模式一致；開啟對話框時不自動建立介面列。

**Blocked by:** 無

**Status:** ready-for-agent

- [ ] `addInterface()`：`interfaceDrafts.value.length === 0` 時 `name: "eth0"`，否則 `name: ""`（檢查須在 push 前）。
- [ ] 不自動建立：`prepare()`／`loadInterfaces()` 不新增任何預填列；Kea 租約的 `prefillInterface` 行為不變。
- [ ] 清單非空（含刪除後仍有列）時按鈕不預填。
- [ ] `pnpm typecheck`、`pnpm lint:check`、`pnpm build` 通過。

## Comments
