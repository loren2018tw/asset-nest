# 04 — 前端：登出按鈕與右上角資訊（Loren／GitHub／版本）

Status: done
Blocked by: 03

## 目標

實作 `spec §5.3`：右上角取代 `Quasar v{{ $q.version }}`，加登出按鈕。

## 範圍

- `frontend/src/layouts/MainLayout.vue`：
  - 移除 `<div>Quasar v{{ $q.version }}</div>`。
  - 右側叢集（`row items-center`）：
    1. `Loren`（純文字，小字）。
    2. GitHub 圖示：`q-btn`（flat dense round，`type="a"`、`:href="'https://github.com/loren2018tw/asset-nest'"`、`target="_blank"`、`rel="noopener"`、aria-label「GitHub 專案首頁」）；圖示用 **inline SVG**（GitHub mark、`fill="currentColor"`、約 18–20px），不新增 icon font。
    3. `V{version}`（`text-caption`）；`onMounted` 用既有 `fetchHealth()`（`@/api/health`）取得 `version`，失敗時不顯示版本文字。
    4. `q-separator vertical`。
    5. 登出：`q-btn flat dense round icon="logout"`，aria-label＋tooltip「登出」；點擊呼叫 auth store `logout()`——成功 `$q.notify`「已登出」＋`router.replace("/login")`；失敗 `$q.notify`「登出失敗，請重試」（維持登入狀態）。
- `frontend/package.json`：`version` 改 `0.1.0`（對齊；真實來源為 `backend/Cargo.toml`，見 ADR-0021）。

## 驗收

- `pnpm --filter frontend typecheck`、`pnpm lint:check`、`pnpm build:frontend`。
- 人工檢核：登入後右上角顯示 `Loren`＋GitHub 圖示（點擊開新視窗到 `https://github.com/loren2018tw/asset-nest`）＋`V0.1.0`＋登出；點登出 → 通知「已登出」→ 轉登入頁；重新登入正常。

## 注意

- 不改 `quasar.config.ts` 的 extras（不加 fontawesome／mdi）。
- 不要 `git commit`；不要動 `.scratch/` 內其他票。

## Comments

- 2026-10-07 完成（實作 subagent）。
- 變更摘要：
  - `frontend/src/layouts/MainLayout.vue`：移除 `Quasar v{{ $q.version }}`；右上角叢集改為 `Loren`（`text-caption`）＋ GitHub inline SVG 連結（`q-btn` `type="a"`／`flat dense round`、`target="_blank"`、`rel="noopener"`、aria-label「GitHub 專案首頁」，20px、`fill="currentColor"`）＋ `V{version}`（`text-caption`，`onMounted` 以既有 `fetchHealth()` 取得，失敗不顯示）＋ `q-separator vertical` ＋ 登出鈕（`icon="logout"`、aria-label＋tooltip「登出」）。登出成功 `$q.notify`「已登出」＋ `router.replace("/login")`；失敗 `$q.notify`「登出失敗，請重試」並維持登入狀態。
  - `frontend/package.json`：`version` `0.0.1` → `0.1.0`（對齊 `backend/Cargo.toml`，見 ADR-0021）。
- 驗收指令結果（於 repo 根目錄執行，全數 exit 0）：
  - `pnpm --filter frontend typecheck` → 通過（`vue-tsc --noEmit` 無錯誤）。
  - `pnpm lint:check` → 通過（oxfmt「All matched files use the correct format」、oxlint 無錯誤）。
  - `pnpm build:frontend` → 通過（`Build succeeded`，輸出 `frontend/dist/spa`）。
- 待人工／整合驗證（待票 06）：需瀏覽器＋後端工作階段，登入後右上角顯示 `Loren`＋GitHub 圖示（點擊開新視窗至 `https://github.com/loren2018tw/asset-nest`）＋`V0.1.0`＋登出鈕；點登出 → 通知「已登出」→ 轉登入頁；重新登入正常。
- 偏離說明（均為範圍內細節）：
  - `q-separator vertical` 加 `class="q-mx-sm"` 保留左右間距（票面未指定；純樣式）。
  - GitHub SVG 加 `aria-hidden="true"`，避免與按鈕 aria-label 重複朗讀。
  - 「已登出」通知採 `type: "positive"`（票面未指定類型，沿用 repo 既有成功通知慣例）。
- 未 `git commit`；未動 `.scratch/` 內其他票與本票範圍外檔案（使用者 WIP 未觸碰）。
