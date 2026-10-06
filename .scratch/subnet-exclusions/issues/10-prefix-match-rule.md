# 10 — 前綴搜尋規則調整：完整位址也展開最後一段

Status: done
Blocked by: —

## 目標

使用者回報：IP 清單搜尋與指派 IP 輸入框輸入 `10.1.1.6` 時，應同時命中
`10.1.1.6` 與 `10.1.1.60–69`；目前只會出現 `10.1.1.6`（IP 清單走完整位址
精確捷徑；指派候選把四段視為完整位址）。

## 範圍

- **指派候選端點**（`GET /api/v1/ip-candidates`，spec §8）：無結尾點時最後一段
  一律為前綴段（四段亦然）——`10.1.1.6` → 完整 octet `[10,1,1]`、前綴段值
  {6, 60–69}；`10.1.1.6.`（結尾點）維持完整位址精確。`query_status` 仍以完整
  位址文字判定（不受影響）。
- **候選取樣順序**：含前綴段（多值）時改為值輪流取樣（每值先取一筆、再回到
  首值），確保 `limit`（前端 20）內每個前綴段值都出現——`10.1.6` 會是
  `10.1.6.0, 10.1.60.0, …, 10.1.69.0, 10.1.6.4, …`；無前綴段（結尾點）維持
  位址升冪。
- **IP 清單搜尋**（`GET /subnets/{id}/ips?q=`，asset-ip-management spec §4.3）：
  移除「完整 v4 位址精確比對」捷徑；完整 v4 位址改以位址文字前綴比對
  （`10.1.1.6` 命中 `10.1.1.6`、`10.1.1.60–69`），其餘欄位（資產描述／位置／
  MAC／介面名稱）與非完整位址關鍵字維持子字串。狀態／觀測／排序／分頁與出界
  列行為不變。
- 前端 `parseIpv4Prefix`：`completeOctets` 語意更新（無結尾點時最後一段為前綴
  段；僅供「達 2 個完整 octet 才查詢」判定，行為不變）。
- 文件：`spec.md` §8、`.scratch/asset-ip-management/spec.md` §4.3。

## 驗收

- `cargo fmt --manifest-path backend/Cargo.toml -- --check`。
- `cargo test --manifest-path backend/Cargo.toml` 全綠（含新增／調整案例）。
- `pnpm lint:check`、`pnpm --filter frontend typecheck` 全綠。
- 人工（實際資料庫）：`10.1.1.6` 在 IP 清單搜尋與指派輸入框都出現 11 筆
  （`.6` 與 `.60–.69`）；`10.1.6` 在指派輸入框前 20 筆涵蓋所有前綴段值。

## 注意

- 不要 `git commit`。

## Comments

實作摘要（未 commit，依指示）：

- `backend/src/ip_candidates.rs`：`Prefix::parse` 僅在結尾帶點時視所有段為完整 octet；否則最後一段一律為前綴段（四段亦然）。`Prefix::addresses` 改為前綴段值輪流取樣（`index % 值數` 取值、`index / 值數` 為自由位址位移）；無前綴段以單一空值退化為位址升冪。`query_status` 不變（完整 v4 位址文字判定）。
- `backend/src/ips.rs`：移除預設排序的「完整位址精確」捷徑；`matches_query` 對完整 v4 位址改以位址文字前綴比對、帶結尾點的四段為精確比對（與候選一致），其餘維持子字串（含其他欄位）。
- 前端 `frontend/src/utils/cidr.ts`：`parseIpv4Prefix` 的 `completeOctets` 改為「結尾帶點時為段數、否則段數−1」（僅供 ≥2 完整 octet 判定；行為不變）。
- 文件：`spec.md` §8（前綴語意＋取樣順序）、`.scratch/asset-ip-management/spec.md` §4.3（IP 清單搜尋規則）。
- 測試：`src/ip_candidates.rs` 單元（四段前綴展開、輪流取樣順序、升冪退化）、`tests/ip_candidates.rs`（`complete_address_query_expands_last_octet`、`prefix_semantics_stay_within_octet_boundaries` 補輪流取樣案例）、`src/ips.rs` 與 `tests/ips.rs`（前綴展開、結尾點精確、分頁排序）。
- 驗收：`cargo fmt --check` 綠；`cargo test` 全綠（0 failed）；`pnpm lint:check`、`pnpm --filter frontend typecheck`、`pnpm build:frontend` 綠。實機（複製實際 DB、127.0.0.1:18080）：`subnets/1/ips?q=10.1.1.6` 回 11 筆（`.6`、`.60–.69`）；`?q=10.1.1.6.` 回 1 筆；`ip-candidates?q=10.1.1.6` 回 11 筆＋`query_status=available`；`q=10.1.6` 前 20 筆涵蓋 `.6` 與 `.60–.69`。
