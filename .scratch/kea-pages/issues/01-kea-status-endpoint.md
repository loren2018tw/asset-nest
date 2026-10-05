# 01: 後端 Kea 系統狀態端點

**What to build:** `kea::http::Client` 新增 `config_get_dhcp4()`（interfaces／lease-backend／subnets；`kea_subnets()` 改為重用、行為不變）、`status_get()`（pid／uptime／reload；sockets 若有）與 `base_url()`（去 userinfo 的顯示用 URL）；新增 `GET /api/v1/kea/status`：一律 200、分區容錯（三命令獨立嘗試、並行），回 `configured`／`reachable`／`url`／`version`／`interfaces`／`runtime`／`dhcp4`（含本地受管網段數）／`errors`。補 stub 整合測試與真機 `#[ignore]` 測試（唯讀）。詳見 `.scratch/kea-pages/spec.md`。

**Blocked by:** None (can start immediately)

**Status:** done

- [x] 未設定 `KEA_API_URL`：200、`configured=false`、其餘 null、不發命令
- [x] 三命令各自嘗試；`reachable` 依 `version-get`；部分失敗時成功區塊保留、`errors` 記錄失敗 key
- [x] `interfaces` 空陣列＝未監聽；`dhcp4.managed_subnet_count` 取本地 `kea_subnet_id IS NOT NULL` 數
- [x] `kea_subnets()` 行為不變（既有完整同步測試全綠）
- [x] stub 測試：未設定、正常、部分失敗、空 interfaces
- [x] 真機測試（`#[ignore]`）：唯讀實測 status-get 欄位，不 `config-write`

## Comments

實作完成（commit `7a3e18e`，`01 Kea 檢視：後端系統狀態端點（client＋API＋stub／真機測試）`）。

- `backend/src/kea/http.rs`：新增 `Dhcp4Config`（interfaces／lease-backend／subnets）、`StatusInfo`＋`SocketStatus`、`config_get_dhcp4()`、`status_get()`、`base_url()`（去 userinfo 與尾斜線）；`kea_subnets()` 改為重用 `config_get_dhcp4()`（行為不變，既有 `kea_sync.rs` 8 測試全綠）。三者皆唯讀：不帶 `arguments`／`operation-target`、不呼叫 `config-write`。
- `backend/src/api/kea.rs`：`GET /api/v1/kea/status` 一律 200；未設定 `KEA_API_URL` 回 `configured=false`、`reachable=false`、其餘 null（不經會回 400 的 `client()`、不發任何命令）；已設定則三命令 `tokio::join!` 並行，`reachable` 依 `version-get`，失敗區塊 null 並記入 `errors.{version,config,status}`（皆空時省略 `errors`）；`dhcp4.subnet_count`＝`subnet4` 筆數、`managed_subnet_count`＝本地 `SELECT COUNT(*) FROM subnets WHERE kea_subnet_id IS NOT NULL`、`lease_backend`＝`lease-database.type`；`interfaces: []` 照常回空陣列（未監聽、非錯誤）。
- 測試：新增 `backend/tests/kea_view.rs`（stub Kea 處理 version-get／config-get／status-get，回應格式 `[{result, text, arguments}]`）5 測試——未設定（200＋stub 零命令）、正常（含 managed_subnet_count，測試 DB 插入帶／不帶 `kea_subnet_id` 網段各一）、部分失敗（config＋status 失敗、version 保留；version 失敗、其他區塊保留）、空 interfaces＋缺欄位（`sockets` null）。`backend/tests/kea_connectivity.rs` 新增 `#[ignore]` 唯讀實測 `status_commands_against_live_server`（version-get／config-get／status-get，不送任何修改命令）。
- 真機實測（`pnpm test:kea --nocapture`，3 passed／0 failed；`http://10.1.0.2:8000`、Kea 3.2.1）：
  - `base_url`：`http://10.1.0.2:8000`（去 userinfo、無尾斜線）。
  - `version-get`：`text="3.2.1"`、無 `arguments.version`（僅 `arguments.extended` 完整建置資訊）→ API `version` 區塊為 `{"version": null, "text": "3.2.1"}`（前端顯示宜以 `text` 為後備）。
  - `config-get`：`interfaces=[]`（未監聽）、`lease-database.type="memfile"`、`subnet4` 2 筆（id 1／2）。
  - `status-get` 實際欄位：`pid=17489`、`uptime=15462`、`reload=15462`（`uptime`／`reload` 皆為相對秒數，非 epoch 時間）、`sockets={"status":"ready"}`（**物件，非綁定清單**）；另有 `csv-lease-file`／`dhcp-state`／`multi-threading-enabled`／`packet-queue-*`／`thread-pool-size` 等未取用欄位。
  - 既有 version-get 與保留 roundtrip 亦通過（roundtrip 執行後已刪除測試保留、未呼叫 config-write）。
- spec 已依實測以最小幅度更新（「後端」與「待實測定案（真機）」）：`sockets` 由 `Vec<Socket>` 改為 `Option<SocketStatus>`（物件）、`reload` 註明為相對秒數、`version` 區塊真機值註記。**注意**：spec 前端段「reload（本地時間）」與「sockets 有值時並列實際綁定」與實測不符（reload 為相對秒數、sockets 只有狀態），留待票 03 依此調整。
- 驗收：`cargo fmt --check` 綠；`cargo test --manifest-path backend/Cargo.toml` 全綠（96 單元＋127 整合、共 223 passed、3 ignored、0 failed）。
- 實作 commit：`7a3e18e`。
