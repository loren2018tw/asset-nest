# 05 — 整合驗證（全測試與檢核）

Status: done
Blocked by: 01–04

## 目標

對全部票的實作做最終驗證，確認規格落地且無回歸。

## 步驟

1. 讀 `.scratch/asset-disposal/spec.md`、`GLOSSARY.md`「報廢詞彙」與 `docs/adr/0026`，逐條核對。
2. 執行：
   - `cargo fmt --manifest-path backend/Cargo.toml -- --check`
   - `pnpm test`（後端全部整合＋單元）
   - `pnpm --filter frontend typecheck`
   - `pnpm lint:check`
   - `pnpm build:frontend`（SFC 編譯驗證）
3. 以 `git status --short`、`git diff --stat` 檢視變更範圍：不得有未預期的檔案；確認 `backend/migrations/0012_disposed_assets.sql`、`GLOSSARY.md`、`docs/adr/0026`、`.scratch/asset-disposal/` 都在。
4. 交叉檢查：
   - 端點回應欄位與 spec §4 一致（攤平＋`kea_sync`、分頁結構、錯誤碼與訊息）。
   - 不變量：出借中 409、日期驗證、FK 連動、復原原 id 與 `created_at`、介面／指派不回復。
   - 既有功能不受影響：資產清單／搜尋／匯出／清冊稽核自動不含報廢資產、復原後恢復。
5. 端到端煙霧測試（`cargo test` 既有整合測試可取代；或臨時以 `curl` 對 `pnpm dev` 後端）：
   - 建一資產（含介面＋保留指派）→ 由編輯對話框報廢（報廢日期今天）→ 資產清單無該筆、報廢頁有該筆（報廢日期正確）→ 原 IP 回到可用 → 復原 → 資產回來（無介面／指派）→ 報廢頁無該筆 → 重複復原 404。
   - 失敗案例：未來日期 400、缺日期 400、出借中 409、歸還後可報廢。
   - Kea（若有 stub 或實機）：報廢後 Kea 保留消失；推送失敗僅警示、不阻擋。

## 產出

- 驗證報告（回覆）：每項指令結果、發現的問題與修正（如在票 01–04 範圍內可直接修，否則回報）。
- 若有無法通過項目：列出檔案、行號、失敗輸出與建議，不要 `git commit`。

## 注意

- 不要 `git commit`；不要主動改規格或 GLOSSARY（除錯字）。

## Comments

- 2026-10-12 驗證完成（子代理）：逐條核對 spec §1–§8、GLOSSARY「報廢詞彙」與 ADR-0026，全部指令通過、無需修正的失敗項目。詳見報告。
  - `cargo fmt --check`、`pnpm test`（lib 195＋整合 246 全綠，含 `tests/disposal` 7 支）、frontend `typecheck`、`lint:check`、`build:frontend` 全數 EXIT=0。
  - 端到端煙霧測試：以臨時 DB＋`cargo run`（127.0.0.1:18080，已關閉）＋`curl` 登入後跑完整流程——建資產（含介面＋保留指派）→ 缺日期／格式錯／未來日期 400（訊息與 field 正確）→ 出借中 409 → 歸還 → 報廢 201 → 資產 404／清單不含／匯出不含 → 報廢清單含（報廢日期正確）→ 原 IP 可重新指派 → 復原 200（原 id、無介面／指派）→ 報廢清單空、資產清單含、重複復原 404 → 分頁夾 1..=100 → `q` 搜尋與 `%` 字面。Kea 成功／失敗案例由 `tests/disposal.rs` stub 涵蓋（受管網段保留刪除、失敗僅警示不阻擋）。
  - 變更範圍：僅本功能 8 個修改檔＋9 個新增檔（含 `0012_disposed_assets.sql`、`GLOSSARY.md`、`docs/adr/0026`、`.scratch/asset-disposal/`）；未動 `agent/`、`deploy/` 或其他 feature 程式。
  - 觀察（非失敗）：子對話框提示文字以「報廢資產」取代 spec 巢狀引號的『報廢資產』（全站 `『』` 無使用慣例、語意相同，不修）；Kea 推送失敗時保留留在 Kea 待下次完整同步（ADR-0011 既定行為，測試明文斷言）。
