# 01: AssetFormDialog 支援預填指派草稿

**What to build:** `AssetFormDialog` 新增選填 prop `prefillAssignment`（僅新增模式套用，編輯模式忽略）：`prepare()` 時將其置入 `assignmentDrafts` 一筆（結構同 `AssignmentDraftInput`＋內部 `key` 遞增）；與既有 `prefillDescription`／`prefillInterface` 可並用；未提供時行為完全不變。落地沿用既有 `preflightAssignmentDrafts()`／`landAssignmentDrafts()`（含 Kea 推送結果呈現）。詳見 `.scratch/ip-list-new-asset/spec.md`。

**Blocked by:** None

**Status:** ready-for-agent

- [x] `prefillAssignment` prop 型別（`{ address: string; purpose: IpPurpose; hostname: string | null; subnetId: number | null; subnetLabel: string | null } | null`）與文件註解
- [x] `prepare()`（新增模式）置入一筆草稿；與既有 `prepare()` 清空草稿的順序保持一致（先清後填）
- [x] 未提供時行為不變（租約頁、資產頁既有入口不回歸）
- [x] `pnpm --filter frontend typecheck`、`pnpm lint:check` 綠

## 注意

- 草稿一經預填不可編輯（僅能取消重開）——與現行草稿列行為一致，不新增編輯功能。
- 不動 `assignmentDrafts` 驗證／落地邏輯；不動後端。

## Comments

實作完成（主實作 commit `28b88fb`）：

- prop 型別直接沿用 `AssignmentDraftInput | null`（`utils/assignmentDraft.ts`，與草稿結構完全同形，不另造型別）。
- 草稿置入點在 `loadInterfaces()` 新增模式分支（非 `prepare()` 本體）：`rowKey` 須綁在介面草稿列的 `key` 上，置入點與 `prefillInterface` 的草稿建立同一處才不會依賴 async 時序；順序仍為 `prepare()` 先清空、`loadInterfaces()` 後填入。
- 無預填介面但有預填指派草稿時，自動補一筆空 MAC 的 `eth0` 宿主列（MAC 空白＝手動設定介面；ADR-0025 草稿必須有介面列可綁）。
