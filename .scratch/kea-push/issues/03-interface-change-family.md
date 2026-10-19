# 03 — MAC 編輯家族

Status: done
Blocked by: 02

## 目標

`capture_interface_change`／`commit_interface_change`＋module 測試；遷移 `api/interfaces.rs` 更新路徑，移除 route 的 MAC 比較與保留查詢。

## 範圍

- `backend/src/kea/push.rs`：
  - `pub struct PendingInterfaceChange { interface_id: i64, mac_before: Option<String>, reservations: Vec<Assignment> }`（欄位不公開）。
  - `pub async fn capture_interface_change(db, interface_id) -> Result<PendingInterfaceChange, ApiError>`：`interfaces::get`（不存在→`mac_before=None`、保留空）＋`list_reservations_for_interfaces(&[id])`。
  - `pub async fn commit_interface_change(db, kea: Option<&Client>, pending) -> Result<Option<Vec<KeaSync>>, ApiError>`：重讀介面（不存在→`None`）；MAC 未變→`None`；變更→對保留逐筆以新 MAC upsert（網段快取；未受管跳過；介面無 MAC→`KeaError::Data("保留位址 {a} 的介面缺少 MAC")` 記 failed）；空結果→`None`。internal 版本供測試注入 port。
  - 測試：spec §測試決策「MAC」子清單。
- `api/interfaces.rs` PATCH：`capture_interface_change`（在 `interfaces::update` 之前）→ update → `commit_interface_change`；刪除 `if existing.mac != interface.mac` 區塊與 `kea_sync::push_reservation_updates` 呼叫（`existing` 仍供 patch 驗證與清空 MAC 結構規則使用）。
- 舊 `push_reservation_updates` 至此無呼叫者（`pub` 不觸發警告；票 04 退場）。

## 驗收

- `cargo test --manifest-path backend/Cargo.toml --lib kea::push --test kea_sync --test interfaces`
- `cargo test --manifest-path backend/Cargo.toml` 全綠（29 個 kea_sync 測斷言不得改動）
- `cargo fmt --manifest-path backend/Cargo.toml` 後 `-- --check` 綠；`cargo check --all-targets` 無新警告

## 交付

1. commit：`Kea 推送：MAC 編輯家族（票 03）`。
2. 本票檔：`Status: done`、`## Comments` 補 commit hash 與摘要。
3. commit：`Kea 推送：票 03 補記實作與 commit hash`。

## Comments

實作完成（2026-10-19）；commit `a7c6b21`（`Kea 推送：MAC 編輯家族（票 03）`，含 `backend/src/kea/push.rs`、`backend/src/api/interfaces.rs`；2 檔、+298／−16）。

- 變更摘要：
  - `backend/src/kea/push.rs`：
    - `pub struct PendingInterfaceChange { interface_id: i64, mac_before: Option<String>, reservations: Vec<Assignment> }`（欄位私有）。
    - `pub async fn capture_interface_change(db, interface_id)`：`interfaces::get`（不存在→`mac_before = None`）＋`assignments::list_reservations_for_interfaces(&[id])`；DB 錯誤訊息沿用舊 route（「讀取介面失敗」／「讀取保留指派失敗」）；永不 404。
    - `pub async fn commit_interface_change(db, Option<&Client>, pending)`（公開，包 `HttpPush`）／`commit_interface_change_with(db, Option<&P>, pending)`（內部泛型 `Option<&P> where P: KeaPush + ?Sized`，沿用票 01 型式）：重讀介面（不存在→`None`）；MAC 與捕捉值相同→`None`；變更→對保留逐筆（輸入序）網段快取→未受管／無 client 跳過→以新 MAC upsert（介面無 MAC→`KeaError::Data("保留位址 {a} 的介面缺少 MAC")` 記 failed）；結果空→`None`。語意逐字沿用舊 `push_each(refresh = true)`＋`after_assignment_change` 的更新分支。
    - module 測試 +4（共 13）：變更→逐筆 upsert 且順序＝捕捉查詢序、欄位＝新 MAC（跨兩受管網段，查詢序與網段 id 序刻意相反）；MAC 未變（只改備註）→`None`＋零命令；無保留→`None`＋零命令（附介面不存在＝不 404、不推送）；未受管跳過＋無 client 跳過。`update_interface_mac` helper 以 `ValidInterface` 呼叫 `interfaces::update`；既有 9 測的斷言與文字未動。
    - 檔頭 module 文件補 MAC 編輯家族一句。
  - `api/interfaces.rs` PATCH：`kea_push::capture_interface_change`（結構規則驗證後、`interfaces::update` 前）→ update（`None`→404、不 commit）→`kea_push::commit_interface_change`；`kea_sync` 直接取其回傳（`Option<Vec<KeaSync>>`，與舊 route 的 `is_empty` → `None` 相同）。刪除 `if existing.mac != interface.mac` 區塊與 `kea_sync` import；`existing` 仍供 `patch.apply_to` 與「有保留不可清空 MAC」結構規則（該規則的保留查詢保留原狀）。
- 差異與取捨：
  - 捕捉置於結構規則驗證之後、`interfaces::update` 之前：驗證失敗路徑不多做查詢；介面不存在仍由 handler 最早的 `interfaces::get` 回 404（欄位與訊息不變），捕捉本身永不 404。
  - `commit_interface_change_with` 重讀介面一次並以其 MAC 對所有保留共用（舊碼在 `after_assignment_change` 內對同一介面逐筆重讀；值等價）；MAC 比較基準由 handler 讀取時點改為本地異動前（spec 決策 2），正常路徑零差異。
  - 舊 `push_reservation_updates` 至此無呼叫者（`pub mod sync` 仍屬公開 API，無 dead_code 警告；票 04 退場）。
  - 行為、訊息、回應形狀零變更；`backend/tests/kea_sync.rs` 29 測試一字未改。
- 執行過的指令與結果（全綠）：
  - `cargo test --manifest-path backend/Cargo.toml --lib kea::push --test kea_sync --test interfaces` → lib 側 `kea::push` 13 passed、0 failed（整合測試因 filter 為 0）。
  - 同上兩整合目標去 filter 全跑：kea_sync 29、interfaces 5 passed、0 failed。
  - `cargo test --manifest-path backend/Cargo.toml` → 31 個 test 目標合計 485：479 passed、0 failed、6 ignored（`tests/kea_connectivity.rs` 既有需真機 Kea 者；`--lib` 221 passed 含新增 4）。
  - `cargo fmt --manifest-path backend/Cargo.toml` 後 `-- --check` → 通過；`cargo check --manifest-path backend/Cargo.toml --all-targets`（touch 後重編）→ 無警告。
