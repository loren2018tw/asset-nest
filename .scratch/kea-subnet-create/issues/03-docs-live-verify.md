# 03: 文件＋真機驗證——建立缺少網段的流程

**What to build:** `README.md`、`deploy/install.sh` 安裝後提示改為 Kea-first 與 asset-nest-first 並存；`.scratch/kea-subnet-sync/spec.md` 加指向本 feature 的註記；本 spec「實作記錄」彙整三票。真機（`10.1.0.2`）：以 RFC 5737 測試段＋未用 `id` 走完整同步建立 → 驗證（含保留同回合推送、`subnet4-add` 的錯誤訊息形狀）→ `subnet4-del` 清理 → `config-write`（結束後 Kea 無殘留；清理方式於本票決定：測試直送 HTTP 或 client 增方法）。詳見 `.scratch/kea-subnet-create/spec.md`、`docs/adr/0023`。

**Blocked by:** 01、02（真機段需 Kea 可達與使用者同意）

**Status:** done

- [x] README／install.sh 提示：Kea-first 與 asset-nest-first 並存
- [x] 舊 spec（`.scratch/kea-subnet-sync/spec.md`）註記指向本 feature
- [x] 真機建立／驗證／清理（含 `subnet4-del`＋`config-write`）並記錄
- [x] spec 實作記錄；票檔 Comments＋commit（不 push）

## Comments

實作完成（主實作 commit `d026b35`，`03 Kea 網段建立：文件與真機驗證（subnet4-add 建立／清理 roundtrip）`）。

- 文件：
  - `README.md` 安裝後注意（Kea 段）：改為兩種流程並存——Kea-first（先在 `subnet4` 建立並記下 `id`，再回本系統填 `kea_subnet_id`）與 asset-nest-first（在本系統建立／匯入並填 `kea_subnet_id`，Kea 端缺少的受管網段由完整同步以 `subnet4-add` 建立、含位址池與 gateway；**僅增不刪**；指向 ADR-0011／0013／0023）。
  - `deploy/install.sh` `print_summary` 後續指示同步改為兩種流程並存；`bash -n deploy/install.sh` 通過。
  - `.scratch/kea-subnet-sync/spec.md`：在「網段本身不新增／刪除」處加註記（2026-10-07），其「新增」部分已由本 feature（ADR-0023）修訂、「刪除」不變；原文保留。
- 真機測試（`backend/tests/kea_connectivity.rs`，`#[ignore]`；檔頭註解同步更新）：
  - 新增 `create_missing_subnet_roundtrip_against_live_server`：以 `sync::plan`／`apply`（皆 `pub`）＋臨時 in-memory SQLite（套 migrations；直接插入受管網段 `192.0.2.0/24`、測試 gateway、兩段 pool（輸入非數值序）、一筆保留指派）走完整同步。
  - `subnet4-del` 由測試內 reqwest 直送（Basic 認證、`{"command":"subnet4-del","arguments":{"id":…}}`）；未擴大 `Client` 生產介面。
  - 安全：先掃描現有 `subnet-id`（測試 id 取未使用的 max+1）並確認 `192.0.2.0/24` 不存在、`subnet_cmds` 已載入才執行；驗證值先收集、清理完成後才 assert；清理順序 `reservation-del` → `subnet4-del` → `config-write`，最後以 `config-get`／`reservation-get-all` 驗證無殘留；未動既有網段與保留、未編修 Kea 設定檔、未重啟 Kea。
  - 為避免 `pnpm test:kea` 併行時本測試的 `config-write` 把 `reservation_roundtrip` 的暫時保留持久化，兩者以新增的 `LIVE_MUTATION_LOCK`（`tokio::sync::Mutex`）序列化；唯讀測試不受影響。
- 真機結果（`10.1.0.2`、Kea 3.2.1）：
  - 前置：現有 id `[1, 2]`（`10.1.0.0/16`、`140.128.179.0/24`）、`192.0.2.0/24` 不存在；測試 id 3。
  - 計畫：`subnet_add` pools `["192.0.2.10-192.0.2.20", "192.0.2.30-192.0.2.40"]`（正規化、數值排序）、gateway `192.0.2.1`、保留差異 1 筆；`totals.subnet_add=1`、不重複列 pool／gateway 差異。
  - 套用：`subnet_added=true`、`subnet_add_error=None`、保留新增 1／失敗 0、pool／gateway 重複計數 `(0, 0, false)`、`config_write=ok`。
  - 落地驗證：`config-get` 見網段 `192.0.2.0/24`（pools、routers `192.0.2.1`）；`reservation-get-all` 見同回合推送的保留（`192.0.2.5`／`02:00:5e:00:53:99`／`asset-nest-live-test`）。
  - `subnet4-add` 錯誤訊息形狀（cleanup 前對同一 id 重送；狀態不變）原樣：`Kea 回應錯誤（result=1）：ID of the new IPv4 subnet '3' is already in use`。
  - 清理：`reservation-del=Some(true)`、保留已不在 `Some(true)`、`subnet4-del` 成功（`IPv4 subnet 192.0.2.0/24 (id 3) deleted`）、`config-write=true`；清理後 `config-get` 僅 id `[1, 2]`、無測試網段與保留（獨立 curl 再驗證一致）。
- 驗收：`cargo test --manifest-path backend/Cargo.toml` 406 passed／0 failed／6 ignored（新測試編譯並於真機通過；預設 ignored）；`cargo fmt --check` 綠；`pnpm test:kea` 6 passed（含既有 ignored 測試）。
- 未完成或判斷：無。清理方式採「測試直送 HTTP」；`LIVE_MUTATION_LOCK` 為測試檔內機制、不動生產程式碼。spec「實作記錄」已彙整三票（票 01：406 passed／新增 13 測試；票 02：`lint:check`／`typecheck` 綠；票 03：本票結果）。
