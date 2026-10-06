# 05 — 整合驗證（全測試與檢核）

Status: done
Blocked by: 01–04

## 目標

對全部票的實作做最終驗證，確認規格落地且無回歸。

## 步驟

1. 讀 `.scratch/asset-lending/spec.md` 與 `GLOSSARY.md`「借還詞彙」，逐條核對。
2. 執行：
   - `cargo fmt --manifest-path backend/Cargo.toml -- --check`
   - `pnpm test`（後端全部整合＋單元）
   - `pnpm --filter frontend typecheck`
   - `pnpm lint:check`
   - `pnpm build:frontend`（SFC 編譯驗證）
3. 以 `git status --short`、`git diff --stat` 檢視變更範圍：不得有未預期的檔案（例如不應改 `agent/`、`deploy/`）；確認 `backend/migrations/0011_lendings.sql`、`GLOSSARY.md`、`.scratch/asset-lending/` 都在。
4. 交叉檢查：
   - 端點回應欄位與 spec §4 一致（`lending`、`overdue`、分頁結構、`borrowers`）。
   - 資產清單 `lending` 欄位；出借中刪除 409、歸還後可刪、刪除連動清除紀錄。
5. 端到端煙霧測試（`cargo test` 既有整合測試可取代；或臨時以 `curl` 對 `pnpm dev` 後端）：
   - 建一資產 → 借出（借用人「王小明」、預計歸還日為昨天）→ `returned=false` 清單含該筆且 `overdue=true` → 資產清單該列 `lending` 帶值 → 刪除資產 409 → 歸還 → 已歸還清單含該筆 → 刪除資產成功 → lendings 無該資產紀錄。
   - 重複借出 → 409；重複歸還 → 409；borrower 空白 → 400。

## 產出

- 驗證報告（回覆）：每項指令結果、發現的問題與修正（如在票 01–04 範圍內可直接修，否則回報）。
- 若有無法通過項目：列出檔案、行號、失敗輸出與建議，不要 `git commit`。

## 注意

- 不要 `git commit`；不要主動改規格或 GLOSSARY（除錯字）。

## Comments

- 2026-10-07 驗證完成（子代理）：全部指令通過、無需修正的失敗項目。詳見報告。
  - `cargo fmt --check`、`pnpm test`、frontend `typecheck`、`lint:check`、`build:frontend` 全數 EXIT=0。
  - 端到端煙霧測試以既有整合測試取代（tests/lendings、tests/assets、tests/delete_linkage 共 34 個相關測試全過）。
  - 無未預期檔案；未動 `agent/`、`deploy/`；`0011_lendings.sql`、`GLOSSARY.md`、`.scratch/asset-lending/` 均在。
  - 觀察（非失敗）：`GET /lendings?returned=true` 的 `per_page` 夾 1..=100（spec 未明定上限，比照 assets 既有慣例）；出借中清單回應省略 `total/page/per_page` 欄位（spec §4 只定義 `{ items }`，以 `skip_serializing_if` 不序列化）。
