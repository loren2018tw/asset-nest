# 10: 觀測篩選新增「從未上線」

**What to build:** IP 清單「觀測」篩選新增 `never_seen`（UI 顯示「從未上線」）：**有效涵蓋**（該網段有在線觀測代理）下、**已指派**（`static`／`reservation`）且 `last_seen_at` 為空的位址——含已檢查但未見（`last_checked_at` 有值、`last_seen_at` 為空）與尚未檢查者。未指派位址（可用／池內／排除）一律不符；未涵蓋網段（無代理或代理離線）回空集合；v6 維持既有行為（下拉隱藏、帶 `observed` 回空集合）。欄位「最後可見」對未指派未見位址仍顯示「從未上線」，篩選較欄位顯示窄——已接受的取捨，理由見 ## Comments。

**Blocked by:** None (can start immediately)

**Status:** done

- [x] `IpObservedFilter` 新增 `NeverSeen`（查詢值 `never_seen`；白名單訊息更新為 `unassigned_seen、observed_mac_mismatch 或 never_seen`）
- [x] `matches_v4_filters` 實作：有效涵蓋 ∧ 已指派 ∧ `last_seen_at` 為空（篩選與排序路徑共用）
- [x] 前端型別與選項新增「從未上線」
- [x] 單元與整合測試（已指派未見／已見、未指派未見、檢查看空、未涵蓋回空、代理離線、與狀態篩選並用、排序路徑、v6 回空、白名單訊息）
- [x] 文件：`spec.md` §Solution、User Story 與 §讀取端就地更新；`GLOSSARY.md` 不變

## Comments

決策（grilling 定案）：

- **語意採既有「從未上線」**（有效涵蓋下 `last_seen_at` 為空），非「未觀測」（網段層級、整組隱藏另有其道）。用詞不用「未上線」：GLOSSARY 於「最後可見」條目把「上線／在線」列為 Avoid，畫面與對話框既有文字皆為「從未上線」。
- **僅已指派**（`static`／`reservation`）：回答「宣告了卻沒出現的設備」，避免整批未被探索的空閒位址灌入結果；未指派位址在「最後可見」欄仍顯示「從未上線」，**篩選較欄位顯示窄為已接受的取捨**。GLOSSARY 對「從未上線」的措辭（「已指派但從未看到顯示…」）亦支持此界線。
- **未涵蓋回空集合**（後端判斷、前端不特判）：未涵蓋網段「從未上線」依定義不成立；與其他篩選篩不到東西的體驗一致。
- 值名 `never_seen`；選項插在既有兩項之後。

實作摘要（commit `5ddf1ae`）：

- 後端：
  - `ips.rs`：`IpObservedFilter` 新增 `NeverSeen`（查詢值 `never_seen`）；`matches_v4_filters` 新分支＝`context.observed && assignment.is_some() && last_seen_at.is_none()`（有效涵蓋 × 已指派 × 未見，含「僅檢查未見」）；單元測試新增 `never_seen_filter_requires_assignment_and_coverage`（預設路徑、排序路徑、未涵蓋回空）與 parse 白名單。
  - `api/ips.rs`：白名單訊息改「觀測篩選須為 unassigned_seen、observed_mac_mismatch 或 never_seen」。
- 前端：`api/ips.ts` 型別與註解；`IpListPage.vue` 選項「從未上線」（`never_seen`）。
- 測試：`tests/ips.rs` 新增 `never_seen_filter_lists_assigned_unseen_only_under_coverage`——.1 已指派未檢查、.2 保留已檢查未見（命中）；.3 已見、.4 可用、.13 池內（不命中）；未涵蓋與代理離線回空；排序分頁與 `status` 並用；`bogus` 400 且訊息列出新值；v6 回空。新增測試輔助 `insert_checked_only`／`cover_with_agent`／`expire_agents`（比照 observation_history 測試）。
- 文件：`spec.md` §Solution、User Story（新增 24，後續重新編號）、§讀取端篩選規則就地更新；`GLOSSARY.md` 不變（沿用既有詞、不開 ADR）。
- 驗證：`cargo fmt --check`、`cargo test`（lib 196 passed；`tests/ips` 26 passed；全庫 0 failed）、`pnpm --filter frontend lint:check`、`pnpm --filter frontend typecheck`、`pnpm --filter frontend build` 全綠。
