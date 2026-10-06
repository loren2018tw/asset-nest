# 07 — 前端：IP 清單狀態與篩選

Status: ready-for-agent
Blocked by: 02

## 目標

實作 `spec §10.2`：IP 清單顯示「排除」狀態、篩選、列操作停用與衝突徽章。

## 範圍

- `frontend/src/api/ips.ts`：`IpStatus` 增加 `"excluded"`；doc comment 更新（可用／池內／排除／手動設定／保留）；`IpEntry.conflicts` 註解補 `IpInExcludedRange`。
- `frontend/src/pages/IpListPage.vue`：
  - `statusOptions`（v4）新增 `{ label: "排除", value: "excluded" }`（順序：可用、池內、排除、手動設定、保留）。
  - `statusLabel`：`excluded` → 「排除」；`statusColor`：`excluded` → `orange`。
  - 操作欄編輯鈕：`row.status === "excluded" && row.assignment === null` → `disable`、tooltip「排除範圍內位址不可指派」（與池內位址同型，pool 判斷維持原樣）。
  - `conflictInfo` 新增 `IpInExcludedRange`：label「排除範圍」、hint「指派的位址落在排除範圍內（僅提示，不阻擋）」。
  - `ObservedOnUnassigned` 的 hint 文案更新為「未指派、非池內且非排除範圍的位址被觀測到有主（非法佔用；僅提示，不阻擋、不自動回收）」。

## 驗收

- `pnpm --filter frontend typecheck`、`pnpm lint:check`。
- 人工檢核（`pnpm dev`，需先有含排除範圍的網段）：
  - 篩選「排除」只列排除位址；徽章顯示「排除」橘色。
  - 排除且未指派的列編輯鈕停用並顯示 tooltip。
  - 「先指派、後加排除範圍」的列顯示「排除範圍」衝突徽章。

## 注意

- 不要動 `IpListPage.vue` 的搜尋／排序／分頁邏輯與其他欄位。
- 不要 `git commit`。
