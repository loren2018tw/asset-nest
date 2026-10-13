# 01: 後端清除觀測紀錄端點

**What to build:** `DELETE /api/v1/observations`：單一交易刪除全部 `ip_presence`（含網段外觀測列）與 `observation_event`，回 200 `{presence_deleted, events_deleted}`；需登入（非白名單端點）；冪等；宣告（指派／保留）、代理登錄與被拒回報完全不受影響。核心函式放 `observation.rs`。

**Blocked by:** None (can start immediately)

**Status:** done

- [x] `observation` 新增 `ClearCounts`＋`clear_observations(pool)`（單一交易、`rows_affected` 計數、冪等、不 `VACUUM`）
- [x] `api/observations.rs` 新增 `DELETE /observations` 路由與 handler（200 JSON；完成記 `tracing::info`）
- [x] 整合測試 `backend/tests/observations_clear.rs`：刪除筆數與兩表清空、宣告／代理／被拒回報不變、冪等、未登入 401、帶 token 200
- [x] `cargo fmt --check`／`cargo test` 全綠

## Comments

實作完成（未 commit；無 migration）。

- `backend/src/observation.rs`：新增 `ClearCounts`（`presence_deleted`／`events_deleted`）與 `clear_observations`（單一交易依序刪除 `observation_event`、`ip_presence`；`rows_affected` 計數；冪等；不 `VACUUM`）；置於 `cleanup_events` 之後。
- `backend/src/api/observations.rs`：`DELETE /observations`（掛 `/api/v1`）→ 200 JSON；完成記 `tracing::info`；非白名單端點，需登入（認證中介層把關）。模組註解改為讀取端點＋清除端點。
- `backend/tests/observations_clear.rs` 3 個整合測試：①種子（網段、資產／介面／指派、代理、被拒回報、現況 3 列含網段外、事件 2 筆）→ 200 筆數正確、兩表清空、其餘 6 表不變；②空庫回零＋重複清除回零（冪等）；③啟用認證：未帶 cookie 401 且不動資料、帶 `sign_token` token 200 並清除。
- 驗收：`cargo fmt --check` 綠；`cargo test` 全綠（0 failed）；`cargo clippy --all-targets` 新檔案無警告。另以真機短命服務＋curl smoke（獨立暫存 SQLite）：未登入 DELETE→401；`POST /agents/observations` 產生 3 現況＋2 事件後 DELETE→200 `{3,2}`、再刪→`{0,0}`；IP 歷史現況 null／事件空、網段外清單空、代理列與網段保留。
