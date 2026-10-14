# 02: 後端：匯入受管網段保留計數

**What to build:** 資產 CSV 匯入維持不推送 Kea；`ImportReport.created` 新增 `managed_reservations`（本次建立於受管網段＝`kea_subnet_id` 有值的保留數），供前端提示完整同步（見 `.scratch/kea-sync-completeness/spec.md`、`docs/adr/0027`）。

**Blocked by:** None

**Status:** ready-for-agent

- [ ] `import::Created.managed_reservations`（usize）
- [ ] `write_plans` 以網段快取計數（`SELECT kea_subnet_id FROM subnets`；僅 purpose＝reservation）
- [ ] 測試：匯入含 MAC 的 v4 到受管／未受管網段之計數
- [ ] `cargo test`、`cargo fmt --check` 全綠

## Comments
