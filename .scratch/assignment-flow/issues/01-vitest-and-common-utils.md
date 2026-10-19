# 01 — vitest 基礎設施與 errors／text 收斂

Status: done
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

- 實作完成，commit：`3b0becb0e5491b266cb040e71ff439ae92d86160`（`指派流程：vitest 基礎設施與錯誤／文字工具收斂（票 01）`）；本票檔變更不隨 commit（留待統一補記）。
- 安裝：`pnpm --filter frontend add -D vitest` → `vitest@^5.0.5`（實跑 5.0.5）；`pnpm --filter frontend test` 可跑。
- 驗收結果（全綠）：
  - `pnpm --filter frontend test`：2 test files、6 tests passed（errors.test.ts 3、text.test.ts 3）。
  - `pnpm --filter frontend typecheck`：`vue-tsc --noEmit` 無錯（exit 0）。
  - `pnpm lint:check`：oxfmt --check 78 檔格式正確、oxlint 無錯（exit 0）。
- 逐檔確認：21 個本地函式本體與新 util **逐字一致**，無任何不一致（含 format）；替換僅增 21 行 import、刪 63 行函式本體＋21 行空行，呼叫端與其他內容未動。
  - `messageOf` 16 檔：pages/ObservationsOutOfSubnetPage.vue、pages/AssetsPage.vue、pages/SubnetsPage.vue、pages/InventoryAuditPage.vue、pages/IpListPage.vue、pages/LendingsPage.vue、pages/DisposedAssetsPage.vue、components/LendingDialog.vue、components/KeaSyncDialog.vue、components/AssetFormDialog.vue、components/AssignmentDialog.vue、components/AssetImportDialog.vue、components/SubnetImportDialog.vue、components/AssignIpDialog.vue、components/SubnetFormDialog.vue、components/ObservationHistoryDialog.vue。
  - `textOrNull` 5 檔：components/LendingDialog.vue、components/AssetFormDialog.vue、components/AssignmentDialog.vue、components/AssignIpDialog.vue、components/SubnetFormDialog.vue。
- 取捨：
  - 格式以 `frontend/oxfmt.config.ts` 實檔為準（`semi: true`、雙引號、printWidth 80、trailingComma none）——repo 現行碼與 oxfmt 皆帶分號，故未採「無分號」寫法；`lint:check` 綠。
  - `vitest.config.ts`：`@` 別名以 `fileURLToPath(new URL("./src", import.meta.url))`；`environment: "node"`、`include: ["src/**/*.test.ts"]`；未引入 jsdom／happy-dom／外掛。
  - README 表格加入新列時一併重排欄寬（僅空白調整），比照先前新增 `test:agent` 的作法。
  - 本地函式位於檔尾者（7 檔）刪除時吸收其前導空行、其餘吸收尾隨空行，皆保持單一空行分隔。
