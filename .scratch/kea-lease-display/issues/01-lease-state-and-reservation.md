# 01: 租約清單狀態正名與保留標示

**What to build:** 租約清單（Kea DHCPv4 唯讀檢視）目前有兩處資訊不足：狀態 2 被簡化顯示「已過期」，但 Kea 3.2 的正式名稱是 `expired-reclaimed`（已過期且被回收、保留在資料庫供 affinity），且 Kea 3.2 的狀態 4 `registered` 現行沒有對應、只能看到原始數字；另外，一筆租約是否對應本地「保留」登記完全看不出來。本票讓狀態顯示與 Kea 定義一致（篩選同步、未知值原樣保留），並在租約列標示該 IP 是否為本地保留：以本地受管網段（`kea_subnet_id` 對應 Kea `subnet-id`）為範圍、位址與 purpose=reservation 的指派相符者標示「保留」；本地無對應網段時不標示（無從判定），不得誤標。

**Blocked by:** None (can start immediately)

**Status:** done

- [x] 狀態 2 正規化為 `expired-reclaimed`，畫面顯示「已過期（已回收）」，篩選選項可選
- [x] 狀態 4 正規化為 `registered`，畫面顯示「已註冊」，篩選選項可選
- [x] 未知狀態值（例：9）維持原值顯示，不在篩選選項內（維持既有行為）
- [x] 受管網段內、位址與本地保留相符的租約列顯示「保留」標記
- [x] 受管網段內、非保留位址的租約列不顯示標記
- [x] 本地無對應網段（未受管）的租約列不顯示標記
- [x] 後端租約端點整合測試（stub Kea＋種入受管網段與保留）涵蓋上述三種保留判定情況與新狀態值
- [x] Kea 真機唯讀測試的狀態斷言與後端正規化單元測試同步更新
- [x] 前端型別與註解同步；`cargo test`（backend）、`pnpm --filter frontend typecheck`、`pnpm lint:check` 全綠

## Comments

實作完成（未 commit）：

- `backend/src/kea/http.rs`：`normalize_state` 依 Kea 3.2 `basicStatesToText` 正規化——數字 2→`expired-reclaimed`、4→`registered`，0／1／3 不變、未知值保留原值；`KeaLease` 與函式註解同步更新。單元測試：`parse_lease_normalizes_fields_and_state` 期望值改 `expired-reclaimed`；`normalize_state_handles_numbers_strings_and_unknown` 新增 state 4 案例並保留未知值 9 案例。
- `backend/src/api/kea.rs`：`KeaLeaseEntry` 新增 `is_reservation: bool`（既有欄位全數保留）。受管網段查詢改為單一 `subnets LEFT JOIN ip_assignments（a.purpose = 'reservation'）`，同時取得 CIDR／名稱與 `(kea_subnet_id, address)` 保留集合；判定需租約的 `subnet_id` 與 `ip_address` 皆存在且精確命中集合，本地無對應受管網段一律 false（不誤標）。
- `backend/tests/kea_view.rs`：`insert_subnet` 改回傳本地網段 id、新增 `insert_assignment` 輔助（自建資產與介面滿足外鍵）；新增整合測試 `leases_mark_reservation_only_for_managed_matching_assignments`，涵蓋三種保留判定（受管＋reservation 相符→true；受管＋static→false；未受管、即使位址有 reservation 指派→false）與新狀態值（state 2／4 正規化、未知 9 原值）；happy path 測試補上 `is_reservation=false` 斷言。
- `backend/tests/kea_connectivity.rs`：真機唯讀測試 `lease4_get_all_against_live_server` 的狀態白名單加入 `expired-reclaimed`／`registered`（`#[ignore]`，本機不執行）。
- `frontend/src/api/kea.ts`：`KeaLease` 新增 `is_reservation: boolean` 欄位；`state` 註解同步為 Kea 3.2 五個狀態（`default`／`declined`／`expired-reclaimed`／`released`／`registered`），未知保留原值。
- `frontend/src/pages/KeaLeasesPage.vue`：狀態篩選選項與 chip `stateInfo` 同步（「已過期（已回收）」`expired-reclaimed`、「已註冊」`registered`；未知值仍顯示原值且不在選項內）；IP 欄於 `is_reservation` 為 true 時顯示紫色「保留」badge（含說明 tooltip，顏色比照 IP 清單頁的保留色）。
- 未動：observation 邏輯（狀態 0 正規化仍為 `default`，`filter_lease_signals` 不受影響）、`deploy/`；未顯示指派對象／資產名稱，`registered` 未納入任何觀測判斷。

驗證（本機實際執行，全綠）：

- `cargo test --manifest-path backend/Cargo.toml`：exit 0，27 個 test binary 全數 `ok`、0 failed（含新增整合測試與更新後單元測試）；另 `cargo fmt --check` 通過。
- `pnpm --filter frontend typecheck`（vue-tsc --noEmit）：exit 0。
- `pnpm lint:check`（oxfmt --check + oxlint）：exit 0。

未在本機驗證：真機 Kea 唯讀測試（`lease4_get_all_against_live_server`，需 `KEA_API_URL` 且為 `#[ignore]`）僅同步更新狀態斷言，待有真機環境時以 `pnpm test:kea` 執行確認。
