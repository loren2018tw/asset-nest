# 09 — 整合驗證（全測試與檢核）

Status: ready-for-agent
Blocked by: 01–10

## 目標

對全部票的實作做最終驗證，確認規格落地且無回歸。

## 步驟

1. 讀 `.scratch/subnet-exclusions/spec.md` 與 `docs/adr/0020-subnet-exclusions.md`，逐條核對驗收項目。
2. 執行：
   - `cargo fmt --manifest-path backend/Cargo.toml -- --check`
   - `pnpm test`（後端全部整合＋單元）
   - `pnpm --filter frontend typecheck`
   - `pnpm lint:check`
   - `pnpm build:frontend`（SFC 編譯驗證）
3. 以 `git status --short`、`git diff --stat` 檢視變更範圍：不得有未預期的檔案（例如不應改 `agent/`、`deploy/`）；確認 `docs/adr/0020-subnet-exclusions.md`、`GLOSSARY.md`、`.scratch/subnet-exclusions/` 都在。
4. 交叉檢查：
   - `GET /subnets/{id}/ips?status=excluded`、`ip-candidates` 端點與 spec §8 回應欄位一致。
   - CSV 匯出 7 欄、含 `#note`；舊 6 欄檔可匯入。
   - Kea 同步計畫不含排除範圍（`tests/kea_sync.rs` 全綠即通過）。
   - `IpInExcludedRange` 出現在 IP 清單徽章與衝突數。
5. 執行一個端到端煙霧測試（可用 `cargo test` 既有整合測試取代，或臨時以 `curl` 對 `pnpm dev` 後端）：
   - 建網段 10.0.9.0/24＋排除範圍 10.0.9.100-10.0.9.120#NAT。
   - 候選搜尋 `10.0.9.1` 不含排除位址。
   - 指派 10.0.9.100 → 400。
   - IP 清單該位址狀態為「排除」；CSV 匯出含該段。

## 產出

- 驗證報告（回覆）：每項指令結果、發現的問題與修正（如在票 01–08 範圍內可直接修，否則回報）。
- 若有無法通過項目：列出檔案、行號、失敗輸出與建議，不要 `git commit`。

## 注意

- 不要 `git commit`；不要主動改規格或 ADR（除錯字）。
