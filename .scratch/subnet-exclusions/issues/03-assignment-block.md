# 03 — 指派阻擋（新指派進排除範圍）

Status: ready-for-agent
Blocked by: 01

## 目標

實作 `spec §7`：新指派不得落在排除範圍內；既有位址更新／移轉不重驗（ADR-0006）。

## 範圍

- `backend/src/assignments.rs`：
  - 新增 `pub(crate) fn is_in_exclusion(subnet: &Subnet, address: Ipv4Addr) -> Result<bool, ApiError>`（比照 `is_in_pool`，解析 `subnet.exclusions`；格式異常視內部錯誤）。
  - `validate_address`（`existing == false`）v4：host 檢查 → pool 檢查 → **排除範圍檢查**；錯誤：「位址 {address} 落在排除範圍內，不可指派」（field `address`）。
  - 更新 `validate_address` doc comment（spec §7 的兩層原則）。
- `backend/tests/assignments.rs`：
  - `PUT /subnets/{id}/ips/{addr}/assignment`：位址在排除範圍 → 400、訊息含「排除範圍」。
  - 資產端 `PUT /assets/{id}/assignments` 新指派同址 → 400（沿用該檔既有資產／介面 setup）。
  - 既有指派「先指派、後 PATCH 加排除範圍、再更新用途（同介面同位址）」→ 200，不阻擋（此為票 02 的衝突標記範圍）。
  - v6 登錄不受影響（回歸斷言）。

## 驗收

- `cargo test --manifest-path backend/Cargo.toml --test assignments`。
- `pnpm test` 全綠；`cargo fmt --check`。

## 注意

- 移轉（`transfer=true`）僅適用「位址已指派給其他介面」；未指派位址的新指派走 `existing == false`，必須被擋。
- 不要 `git commit`。
