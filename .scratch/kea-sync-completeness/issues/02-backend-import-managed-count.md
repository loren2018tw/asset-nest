# 02: 後端：匯入受管網段保留計數

**What to build:** 資產 CSV 匯入維持不推送 Kea；`ImportReport.created` 新增 `managed_reservations`（本次建立於受管網段＝`kea_subnet_id` 有值的保留數），供前端提示完整同步（見 `.scratch/kea-sync-completeness/spec.md`、`docs/adr/0027`）。

**Blocked by:** None

**Status:** done

- [x] `import::Created.managed_reservations`（usize）
- [x] `write_plans` 以網段快取計數（`SELECT kea_subnet_id FROM subnets`；僅 purpose＝reservation）
- [x] 測試：匯入含 MAC 的 v4 到受管／未受管網段之計數
- [x] `cargo test`、`cargo fmt --check` 全綠

## Comments

實作完成（主實作 commit `393bcda`）。

- `Created` 新增 `managed_reservations`；`write_plans` 以 `kea_subnet_ids: HashMap<i64, Option<i64>>` 快取網段受管狀態，逐筆保留累計。
- `tests/import.rs`：原交易測試 `created` 斷言補 `managed_reservations: 0`；新增 `import_counts_reservations_on_managed_subnets`（受管 1 筆 vs 未受管 0 筆）；新增 `create_subnet_with_kea` 測試輔助。
