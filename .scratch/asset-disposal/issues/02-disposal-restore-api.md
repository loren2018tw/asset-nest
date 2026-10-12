# 02 — 報廢／復原 API

Status: done
Blocked by: 01

## 目標

實作 `spec §4`：`backend/src/api/disposed_assets.rs` 三個端點、Kea 推送與整合測試。

## 範圍

- 新增 `backend/src/api/disposed_assets.rs`：
  - `POST /assets/{id}/disposal`：驗證（400）→ 資產存在（404）→ 出借中（409「此資產出借中，請先歸還」）→ Kea 推送 → `disposed_assets::dispose` → 201。
    - Kea：對 `assignments::list_for_asset` 中 `purpose == "reservation"` 者，逐筆（`subnet_id` 快取）以 `subnets::get` 取網段後呼叫 `kea::sync::after_assignment_change(&state.db, state.kea.as_ref(), &subnet, Some(&assignment), None)`；收集 `Some(sync)` 成 `kea_sync`；失敗為 `KeaSync::failed`、不阻擋（ADR-0011）。
    - 回應：`DisposeResponse { #[serde(flatten)] disposed, kea_sync: Vec<KeaSync> }`。
  - `GET /disposed-assets?q=&page=&per_page=`：`{ items, total, page, per_page }`；`page` 由 1、`per_page` 預設 10 夾 1..=100（比照 `api/lendings.rs`）。
  - `POST /disposed-assets/{id}/restore`：200＋`Asset`；404「找不到報廢資產」。
- `backend/src/api/mod.rs` v1 路由 `.merge(super::disposed_assets::router())`（`auth` 與 `import` 之間）。
- 整合測試 `backend/tests/disposal.rs`（比照 `delete_linkage.rs` 的 `test_pool`／`send` helpers，照 spec §4 清單）：
  - 流程：建資產＋介面＋指派 → 報廢 201 → `GET /assets/{id}` 404、清單不含 → 報廢清單含該筆 → 原 IP 可重新指派 → 復原 200（原 id、無介面／指派）→ 清單互換。
  - 驗證：缺日期／`2026/1/1`／未來 → 400（field `disposal_date`）；今天可。
  - 出借中 409；歸還後可報廢；借還紀錄連動刪除。
  - 復原：不存在／重複 → 404。
  - 清單：`q`、`page`／`per_page`、上限夾 1..=100。
  - Kea：受管網段＋保留 → 報廢後 stub 保留消失、`kea_sync` 為 `ok`；stub 失敗時仍 201、`kea_sync` 為 `failed`、本地已搬移（比照 `tests/kea_sync.rs` 的 stub 手法；stub 為該檔私有，可複製最小 stub 或抽共用 helper，於 Comments 記錄選擇）。

## 驗收

- `cargo test --manifest-path backend/Cargo.toml --test disposal`
- `pnpm test`（後端全部）全綠；`cargo fmt --manifest-path backend/Cargo.toml -- --check`。

## 注意

- 錯誤一律走 `ApiError`；訊息照 spec §4。
- 不要 `git commit`；不改前端；不改 01 的 schema（除錯字）。
- Kea 推送順序在搬移之前（搬移後指派已不存在）。

## Comments

### 2026-10-12 完成紀錄（agent）

- 新增 `backend/src/api/disposed_assets.rs`：
  - `POST /assets/{id}/disposal`：驗證（400，field `disposal_date`）→ `assets::get`（404「找不到資產」）→ `lendings::has_open_lending`（409「此資產出借中，請先歸還」）→ Kea 推送 → `disposed_assets::dispose` → 201＋`DisposeResponse`（`DisposedAsset` 欄位攤平＋`kea_sync: Vec<KeaSync>`；未涉同步為空陣列）。
  - Kea 推送：對 `assignments::list_for_asset` 中 `purpose == "reservation"` 者逐筆，`subnet_id` 以 `HashMap` 快取 `subnets::get` 結果後呼叫 `kea::sync::after_assignment_change(..., Some(&assignment), None)`；僅收集 `Some(sync)`；失敗為 `KeaSync::failed`、不阻擋（ADR-0011）；推送在搬移之前。
  - `GET /disposed-assets?q=&page=&per_page=`：`{ items, total, page, per_page }`；`page` 由 1、`per_page` 預設 10 夾 1..=100（比照 `api/lendings.rs`）。
  - `POST /disposed-assets/{id}/restore`：200＋`Asset`；不存在 404「找不到報廢資產」。
- `backend/src/api/mod.rs`：`mod disposed_assets;`（`auth` 與 `error` 之間）與 v1 `.merge(super::disposed_assets::router())`（`auth` 與 `import` 之間）。
- 新增整合測試 `backend/tests/disposal.rs`（7 支）：
  - 流程：全欄位資產＋介面＋static 指派 → 報廢 201（欄位攤平、`kea_sync` 空）→ 資產 404／清單不含 → 報廢清單含（欄位與報廢日期）→ 原 IP 可重新指派 → 復原 200（原 id、無介面／指派）→ 清單互換。
  - 驗證：缺／空白／`2026/1/1`／未來 → 400（field `disposal_date`，訊息照 spec）；失敗不搬移；今天可。
  - 出借中 409；歸還後可報廢；借還紀錄（含已歸還歷史）連動刪除。
  - 復原：不存在／重複 → 404「找不到報廢資產」。
  - 清單：`q` 子字串、`disposal_date DESC`、`page`／`per_page`、夾 1..=100。
  - Kea：受管網段＋兩筆保留（＋一筆 static）→ 報廢後 stub 保留消失、`kea_sync` 兩筆皆 `ok`、逐筆各寫入一次設定檔；stub 失敗（`reservation-del`）仍 201、`kea_sync` `failed`、不寫設定檔、本地已搬移且保留留在 stub（待完整同步修復）。
- 驗收：`cargo test --manifest-path backend/Cargo.toml --test disposal` → 7 passed；`pnpm test` → exit 0、全部測試套件綠（lib 195 passed 等）；`cargo fmt --manifest-path backend/Cargo.toml -- --check` → 通過。

偏離說明：

- **Kea stub 採「複製最小 stub」**：`tests/kea_sync.rs` 的 stub 為該檔私有；為避免動到既有測試，於 `tests/disposal.rs` 內複製僅含 `reservation-add`／`reservation-del`／`config-write` 的最小 stub（記憶體保留、失敗注入、`config-write` 計數），未抽共用 helper、未改 `tests/kea_sync.rs`。
- **Kea 推送需重讀指派**：`assignments::list_for_asset` 回傳資產詳情讀模型 `AssetAssignment`，非 `after_assignment_change` 所需的 `Assignment`；保留列逐筆以 `assignments::find(subnet_id, address)` 重讀後傳入 `Some(&assignment)`（並行異動查無時跳過）。未改領域模組或 schema。
- 測試細節：受管網段測試的 static 指派改用第三個介面（「同一介面同一網段至多一個位址」規則）；`kea_sync` 一律序列化（無推送為 `[]`），照 spec §4 回應型別。

