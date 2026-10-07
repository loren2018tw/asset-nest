# 規格：搜尋欄位注音組字（IME composition）異常修復

- 狀態：已定案（2026-10-07）並完成實作與真 IME 自動化驗證；待使用者 Chrome 手動驗收。
- 本議題為前端輸入法行為修正，未新增領域詞彙（`GLOSSARY.md` 不動）；不開 ADR（可逆、範圍小），取捨理由見 §3。

## 1. 問題現象

使用注音輸入法（Lubuntu + fcitx5-chewing + Chrome 152）於搜尋框輸入時：

- 慢速輸入：顯示正常，但會以預編輯字串（ㄓ、ㄓㄨ、ㄓㄨㄥ）觸發查詢，且「已確認文字」的查詢不會發生。
- 快速輸入（例：欲搜尋「中」）：輸入框殘留預編輯字串「ㄓㄨㄥ」，並以該字串觸發查詢。

受影響：「打字即查」類型欄位——`AssetsPage`、`IpListPage` 搜尋 q-input；`AssignmentDialog`、`AssignIpDialog` 的 use-input q-select；`LendingDialog` 借用人搜尋同型。

## 2. 根因（依碼＋真 IME 實測）

### 2.1 平台實測的組字生命週期（Chrome 152 + fcitx5-chewing）

以 python-xlib XTEST 實際送鍵、於頁面記錄原生事件，觀察到：

- 組字 session 跨詞長存：整段 focus 期間只有開頭一個 `compositionstart` 與**失焦時**一個 `compositionend`；每次選字 commit 是 `compositionupdate(data=轉換後文字)` ＋ `input(value=轉換後文字)`。
- **commit 的 `input` 事件 `isComposing` 亦為 `true`**；整段期間不存在 `isComposing=false` 的 input。
- 失焦時未確認的預編輯會以原始注音字串直接留在欄位（不轉換也不丟棄，與原生 input 行為一致）。
- 因此「組字中不 emit、compositionend 再 flush」的 Quasar 流程在此平台不可用：設起 `qComposing` 會讓已確認文字被扣住、直到失焦才更新（此結論廢棄了原「設 qComposing=true」方案）。

### 2.2 缺陷鏈

1. Quasar 2.34.0 的 `use-key-composition` 只在 `compositionupdate.data` 命中日／中／韓 regex 時設 `qComposing`；注音符號（U+3105–U+312F）與聲調符號不在範圍 → 預編輯字串走一般輸入路徑：debounce 排程、model 寫入與重繪互相糾纏（慢打＝預編輯查詢、快打＝殘留「ㄓㄨㄥ」）。
2. 獨立缺陷鏈：commit 後約 300ms（搜尋 debounce 觸發的頁面重繪）Vue `patchDOMProp` 把 model 值寫回 input；若使用者已開始打下一詞，寫入會蓋掉進行中的預編輯 → Chromium 重設 IME（compositionstart 重啟、字串錯亂如「中中ㄓㄨㄥ」）。此為 Chromium 對「組字期間程式化改值」的既有行為（同 Element UI #14521 類案例）。
3. 上游狀態：Quasar 2026-07-20 曾以 `compositionstart` 為準修復（PR #18385），2026-07-21 被 revert（commit `bf0126`）；master／dev／最新發行版 2.34.0（2026-09-29）皆無此修復 → 升級無解、等上游無時程。

## 3. 決策（2026-10-07）

| 題 | 決策 |
|---|---|
| 修復機制 | app 端組字守衛（見 §4）：以「值尾端仍為注音」判定預編輯，捕獲階段攔下 `compositionupdate`／`input`，並接管原生 input 的 `value` setter，吞掉預編輯期間的程式化寫入 |
| 查詢時序 | q-input 移除 `debounce="300"`（emit 即時，消除寫回空窗），搜尋去抖上移 app 層（`utils/debounce.ts`，仍 300ms） |
| 範圍 | 兩頁搜尋 q-input ＋ `AssignmentDialog`／`AssignIpDialog`／`LendingDialog` q-select。**不**在 fallthrough 加 `isComposing` 檢查：本平台 commit 的 `isComposing` 亦為 true，加了會漏掉 commit；守衛的捕獲攔截已涵蓋。不含 KeaLeases 純客戶端搜尋與一般表單欄位；不修改 Quasar 依賴 |
| 驗收語意 | 組字中：不觸發查詢、不寫 model、不讓重繪蓋掉預編輯；已確認文字：以既有 300ms 去抖查詢一次；快打慢打一致；組字中失焦可恢復 |
| 驗證 | python-xlib → fcitx5-chewing → Chrome 真 IME 自動重現（修前取證、修後回歸）＋使用者 Chrome 手動驗收 |
| 紀錄判定 | 不開 ADR（可逆、範圍小）；`GLOSSARY.md` 不增詞（IME 屬實作細節、非領域詞彙） |

## 4. 守衛設計要點（`frontend/src/composables/useCompositionGuard.ts`）

- 判定式：`/[\u3105-\u312F\u31A0-\u31BF\u02C7\u02C9\u02CA\u02CB\u02D9]$/u`（值／事件資料尾端為注音＝預編輯進行中）。
- `compositionupdate`：資料或欄位值尾端為注音 → `stopPropagation()`（不讓 Quasar 與 Firefox 的泛用判斷設 `qComposing`）。
- `input`：`isComposing === true` 且值尾端為注音 → `stopPropagation()`（不進 Quasar 與 fallthrough 監聽）。
- `value` setter：預編輯中吞掉程式化寫入（Vue 重繪不覆寫預編輯）；瀏覽器／IME 自身編輯不走 JS setter，不受影響。卸載時還原。
- 其他語言 IME 不受影響（資料與值尾端皆非注音即不攔）；未來 Quasar 若正式修復此偵測（觀察 dev 分支），可整段移除守衛。

## 5. 驗證結果（2026-10-07，真 IME 自動化）

修前（重現）與修後（回歸）對照；資產頁與 IP 頁各以實際送鍵「ㄓㄨㄥ→中」驗證：

| 場景 | 修前 | 修後 |
|---|---|---|
| 資產頁快打「中」 | 值「ㄓㄨㄥ」；查詢 q=ㄓㄨㄥ | 值「中」；僅查詢 q=中 |
| 資產頁慢打「中」 | 值「中」；查詢 q=ㄓ、ㄓㄨ、ㄓㄨㄥ（無 q=中） | 值「中」；僅查詢 q=中 |
| 連續兩字「中中」 | IME 被重繪寫回中斷、值「中中中」、查詢僅 q=ㄓㄨㄥ | 值「中中」；查詢 q=中 → q=中中；無 IME 重啟 |
| IP 頁快打「中」 | （同缺陷鏈，未另測） | 值「中」；僅查詢 q=中 |

- `pnpm typecheck`、`pnpm lint:check`、`pnpm build:frontend` 全數通過。
- 重現工具（CDP 腳本＋XTEST 送鍵）暫存於 `/tmp/opencode/ime-repro/`；是否納入 repo 測試基建另議。
