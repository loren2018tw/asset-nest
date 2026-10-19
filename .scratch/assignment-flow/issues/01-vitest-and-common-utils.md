# 01 — vitest 基礎設施與 errors／text 收斂

Status: todo
Blocked by: —

## 目標

依 spec §決策 6／7、§範圍票 01：為 frontend 裝上 vitest（node 環境），並把逐字複製的 `messageOf`（16 檔）與 `textOrNull`（5 檔）收斂成共用 util。

## 範圍

- `frontend/package.json`：以 `pnpm --filter frontend add -D vitest` 安裝（鎖進 `pnpm-lock.yaml`）；scripts 加 `"test": "vitest run"`。
- 新增 `frontend/vitest.config.ts`：`@` → `./src` 別名；`test.environment: "node"`；`test.include: ["src/**/*.test.ts"]`；不引入 jsdom／happy-dom。
- 根 `package.json`：scripts 加 `"test:frontend": "pnpm --filter frontend test"`；`test`（cargo）不動。
- `README.md`「常用指令」表加一列 `pnpm test:frontend`（前端單元測試）。
- 新增 `frontend/src/utils/errors.ts`：`messageOf(cause: unknown): string`；繁中 JSDoc（比照 `utils/` 既有風格，引用本 spec）。
- 新增 `frontend/src/utils/text.ts`：`textOrNull(value: string): string | null`；繁中 JSDoc。
- 新增測試 `frontend/src/utils/errors.test.ts`、`frontend/src/utils/text.test.ts`。
- 全檔替換（刪除本地函式、改 import；呼叫端不變）：
  - `messageOf`：`pages/ObservationsOutOfSubnetPage.vue`、`pages/AssetsPage.vue`、`pages/SubnetsPage.vue`、`pages/InventoryAuditPage.vue`、`pages/IpListPage.vue`、`pages/LendingsPage.vue`、`pages/DisposedAssetsPage.vue`、`components/LendingDialog.vue`、`components/KeaSyncDialog.vue`、`components/AssetFormDialog.vue`、`components/AssignmentDialog.vue`、`components/AssetImportDialog.vue`、`components/SubnetImportDialog.vue`、`components/AssignIpDialog.vue`、`components/SubnetFormDialog.vue`、`components/ObservationHistoryDialog.vue`。
  - `textOrNull`：`components/LendingDialog.vue`、`components/AssetFormDialog.vue`、`components/AssignmentDialog.vue`、`components/AssignIpDialog.vue`、`components/SubnetFormDialog.vue`。
- 逐一確認本地函式本體與新 util **逐字一致**；有任何不一致就停下來回報，不擅自統一語意。

## 驗收

- `pnpm --filter frontend test` 綠（errors／text 測試）。
- `pnpm --filter frontend typecheck` 綠。
- `pnpm lint:check` 綠。

## 交付

1. commit：`指派流程：vitest 基礎設施與錯誤／文字工具收斂（票 01）`。
2. 本票檔：`Status: done`，`## Comments` 補記 commit hash、測試數與取捨（含逐檔確認結果）。

## Comments
