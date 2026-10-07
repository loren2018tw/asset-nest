# 01: 注音組字守衛（搜尋輸入框）

**What to build:** 為「打字即查」欄位補上注音組字防護，修正快速輸入注音時殘留預編輯字串（如「ㄓㄨㄥ」）與組字期間誤觸查詢的問題（規格見 `../spec.md`）。

**Status:** done

- [x] 新增 `frontend/src/composables/useCompositionGuard.ts`：捕獲攔截 `compositionupdate`／`input`（值尾端注音＝預編輯）＋接管 `value` setter（預編輯中不讓重繪覆寫）；卸載還原
- [x] 掛載：`AssetsPage.vue`、`IpListPage.vue`（搜尋 q-input）；`AssignmentDialog.vue`、`AssignIpDialog.vue`、`LendingDialog.vue`（q-select）
- [x] q-input 移除 `debounce="300"`、新增 `utils/debounce.ts` 將搜尋去抖上移 app 層（`onSearchInput`）
- [x] 真 IME 重現：修前取證、修後回歸（見 Comments；fallthrough 的 `isComposing` 檢查改由守衛涵蓋，原因見 spec §3）
- [x] `pnpm typecheck`、`pnpm lint:check`、`pnpm build:frontend` 全綠
- [ ] 使用者 Chrome 快打驗收

## Comments

實作完成（2026-10-07）。真 IME 實測環境：Lubuntu + fcitx5-chewing + Chrome 152（python-xlib XTEST 送鍵、CDP 記錄事件與請求）。

**修前（重現）**

- 快打「中」：值殘留「ㄓㄨㄥ」、查詢 `q=ㄓㄨㄥ`。
- 慢打「中」：值正確但查詢 `q=ㄓ`、`q=ㄓㄨ`、`q=ㄓㄨㄥ`（「中」的查詢未發生）。
- 連續兩字「中中」：commit 後 300ms 的頁面重繪把 model 寫回 input、蓋掉下一詞預編輯 → Chromium 重設 IME；值錯亂「中中ㄓㄨㄥ」。

**修後（回歸）**

- 快打／慢打「中」：值「中」、僅查詢 `q=中`；IP 頁同。
- 連續兩字「中中」：值「中中」、查詢 `q=中` → `q=中中`；無 IME 重啟。

**與原計畫差異（實測推翻，詳見 spec §2、§3）**

1. 原「compositionstart 設 `qComposing=true`」方案廢棄：本平台整段 focus 只有失焦時一次 compositionend，設旗標會鎖住更新直到失焦。
2. 原「fallthrough 補 `isComposing` 檢查」廢棄：commit 的 `isComposing` 亦為 true，加了會漏掉 commit；守衛的捕獲攔截已涵蓋 fallthrough。
3. q-input 的 `debounce` 移除、搜尋去抖上移 app 層：消除「commit 後 300ms 才 emit」的寫回空窗。

**檔案**

- 新增：`frontend/src/composables/useCompositionGuard.ts`、`frontend/src/utils/debounce.ts`
- 修改：`AssetsPage.vue`、`IpListPage.vue`、`AssignmentDialog.vue`、`AssignIpDialog.vue`、`LendingDialog.vue`

**後續**

- 使用者 Chrome 手動快打驗收（快打／慢打／連續多字／各對話框欄位）。
- （可選）整理 Quasar 上游 issue 草稿（含本平台生命週期實測與 revert 脈絡）。
- 重現工具暫存 `/tmp/opencode/ime-repro/`（CDP driver＋XTEST 送鍵＋場景腳本）；是否納入 repo 測試基建另議。
- 實作 commit：未提交（待確認）。
