# Kea 網段建立：完整同步補建缺少的受管網段

對應票號：01（後端差異與建立）、02（前端對話框與表單）、03（文件＋真機驗證）。

決策見 `docs/adr/0023`；延伸 `docs/adr/0013`（網段層設定同步）與 `docs/adr/0011`（計畫與失敗策略）。詞彙依 `GLOSSARY.md`。

## 目標

- 受管網段（v4＋`kea_subnet_id`）在 Kea 端沒有對應 `subnet-id`、且無相同 CIDR 掛於其他 `id` 時，完整同步以 `subnet4-add` 建立：`id`、CIDR、位址池、gateway 一次帶入。
- 先產生唯讀計畫、確認後套用；計畫與結果可見「新增網段」。

## 範圍

- 僅受管網段；僅在完整同步（網段 PATCH 不即時推送，同 ADR-0013）。
- **僅增不刪**：Kea 端多出的網段、asset-nest 刪除或改號後遺留的網段，一律不自動刪除。
- 配對以 `kea_subnet_id` 為鍵（CIDR 比對沿用 `same_network` 正規化）：
  - `id` 不存在、CIDR 無同者 → 新增。
  - `id` 不存在、相同 CIDR 掛於其他 `id` → 計畫預檢錯誤（訊息指出占用者 `id`），不送 `subnet4-add`。
  - `id` 存在但 CIDR 不符 → 維持整段錯誤（不修改、不刪除 Kea 既有網段）。
- 建立內容僅 `id`／`subnet`／`pools`／`option-data` 的 `routers`；不帶 reservations；其餘參數依 Kea 全域繼承。
- 受管但全空的網段（無池、無 gateway、無保留）仍建立；衝突標記不阻擋建立（僅影響該筆保留推送）。
- shared network 不在本系統模型內：同 CIDR 會由預檢擋下；不會把新網段加入 shared network。
- IPv6 與未受管網段完全不變。

## 後端

### client（`kea::http::Client`）

- `subnet4_add(subnet: &Value)`：`subnet4-add`（`arguments.subnet4 = [物件]`；subnet_cmds hook）。修改命令；成功後由完整同步統一 `config-write`。
- 建立物件組裝（在 `kea::sync`）：`{"id": …, "subnet": cidr}`＋`pools`（`{"pool": to_kea_string}`、數值排序）＋`option-data` 的 routers 條目（形狀同 update 路徑：`{"name":"routers","code":3,"space":"dhcp4","data":…}`；未設 gateway 則不帶）；空 `pools` 不帶。

### `kea::sync`

- 計畫（`GET /api/v1/kea/sync/plan`）：
  - 每網段新欄位 `subnet_add`（物件，僅存在時序列化）：`{"pools": ["start-end", …], "gateway": "…" | null}`。
  - `totals` 增 `subnet_add`（筆數）。
  - 缺少網段：`subnet_add` 帶期望值；其保留差異照算（Kea 端視為空集合，不呼叫 `reservation-get-all`）；`pool_add`／`pool_delete`／`gateway` 不重複填。
  - 同 CIDR 掛其他 id：`error = "相同 CIDR 已由 Kea 網段 id {n} 使用"`；不計 `subnet_add`。
  - `id` 相符但 CIDR 不符：`error` 不變（不回歸）。
- 套用（`POST /api/v1/kea/sync`，重算計畫後）：
  - 順序：① 逐網段建立缺少者（序列；含池與 gateway）→ ② 保留三相位（含新建網段的保留）→ ③ 既有網段網段層設定（新建者跳過）→ ④ 任一成功變更才 `config-write` 一次。
  - 建立失敗：記 `subnet_add_error`、跳過該網段保留與設定、不阻擋其他網段。
  - 報告每網段增 `subnet_added: bool` 與 `subnet_add_error?: string`；新建網段不另計 `pool_added`／`gateway_updated`（已含於建立）。

### `subnets.rs`／`subnet_import.rs`

- `kea_subnet_id` 範圍驗證：整數且 `0 < id < 4294967295`（Kea 限制）；v4 限定與全系統唯一不變；CSV 匯入比照。

## 前端

- `api/kea.ts`：`KeaPlanSubnet.subnet_add?: { pools: string[]; gateway: string | null }`；`KeaSyncPlan.totals.subnet_add: number`；`KeaApplySubnet.subnet_added: boolean`、`subnet_add_error?: string`。
- `KeaSyncDialog.vue`：
  - `canApply` 納入 `totals.subnet_add`。
  - 計畫 banner 增「新增網段」計數；每網段列加註「將建立 Kea 網段」；明細增「新增網段」段落（位址池逐段、gateway 值；`null` 顯示「（未設）」）；`hasDetail()` 納入 `subnet_add`。
  - 結果 banner 增「已建立網段」計數；`subnet_added` 於網段列顯示；`subnet_add_error` 以 negative 顯示（「建立網段失敗：…」）、通知視為部分失敗。
- `SubnetFormDialog.vue`：hint 改「選填；僅 IPv4，全系統唯一；Kea 尚無此 subnet-id 時，完整同步會建立」；rule 加範圍（`0 < id < 4294967295`）。
- `SubnetImportDialog.vue`（第 7 欄說明）比照補上「可由完整同步建立」與範圍。
- `KeaStatusPage.vue`：「Kea 的網段數與本地受管網段數不一致」提示鬆綁（可能只是尚未同步）。

## 測試

- 單元（`kea::sync`／`kea::http`／`subnets`）：建立物件組裝（有無池／gateway）、同 CIDR 預檢、id 範圍。
- stub 整合（`tests/kea_sync.rs`，stub 需支援 `subnet4-add`；`id` 重複或前綴重複比照真機回錯）：
  - 計畫：缺網段 → `subnet_add`＋保留差異；同 CIDR 他 id → error；id 相符 CIDR 不符 → error（不回歸）。
  - 套用：建立 → 同回合推保留 → 單次 `config-write`；建立失敗 → `subnet_add_error`＋該網段保留跳過＋他網段續行＋無成功變更不寫檔；既有網段流程不回歸。
- 真機（`pnpm test:kea`，`#[ignore]`，票 03）：RFC 5737 測試段＋未用 `id` 建立 → 驗證（含保留同回合推送）→ `subnet4-del` 清理 → `config-write`；清理方式由票 03 定（測試直送 HTTP 或 client 增方法）。
- 前端：無測試基礎設施（typecheck＋lint 把關）。

## 指令

```sh
cargo test --manifest-path backend/Cargo.toml
pnpm test:kea
pnpm lint:check
pnpm --filter frontend typecheck
```

## 實作記錄

- 票 01（後端）：主實作 commit `d022a89`（`01 Kea 網段建立：後端 subnet4-add 與 sync 建立缺少網段（含 stub 測試）`）。`kea::http::Client::subnet4_add`（`arguments.subnet4=[物件]`）；`kea::sync` 計畫 `subnet_add`／`totals.subnet_add`、同 CIDR 預檢（`same_cidr_owner`）、缺少網段不呼叫 `reservation-get-all`；套用「建立（序列）→ 保留三相位 → 既有網段設定 → 單次 `config-write`」，建立失敗記 `subnet_add_error`、孤立並續行、不重複計 pool／gateway；`kea_subnet_id` 範圍 `0 < id < 4294967295`（API＋CSV 匯入）。驗收：`cargo test --manifest-path backend/Cargo.toml` 406 passed／0 failed／5 ignored（新增 13 測試）；`cargo fmt --check` 綠。
- 票 02（前端）：主實作 commit `4bf3146`（`02 Kea 網段建立：前端「新增網段」呈現與表單提示`）。`api/kea.ts` 型別（`KeaSubnetAddPlan`／`subnet_add?`／`totals.subnet_add`／`subnet_added`／`subnet_add_error?`）；`KeaSyncDialog` 計畫與結果「新增網段」呈現、`canApply` 納入、部分失敗通知；`SubnetFormDialog`／`SubnetImportDialog` hint 與範圍；`KeaStatusPage` 不一致提示鬆綁。驗收：`pnpm lint:check`、`pnpm --filter frontend typecheck` 綠。
- 票 03（文件＋真機驗證）：主實作 commit `d026b35`（`03 Kea 網段建立：文件與真機驗證（subnet4-add 建立／清理 roundtrip）`）。`README.md`、`deploy/install.sh` 安裝後提示改 Kea-first／asset-nest-first 並存（`bash -n` 通過）；`.scratch/kea-subnet-sync/spec.md` 加註記；`tests/kea_connectivity.rs` 新增 `#[ignore]` 真機建立 roundtrip（`sync::plan`／`apply`＋臨時 in-memory SQLite；清理 `reservation-del` → `subnet4-del`（測試內 reqwest 直送）→ `config-write`）與變更型測試序列鎖；檔頭註解更新。
- 驗收：`cargo test --manifest-path backend/Cargo.toml` 406 passed／0 failed／6 ignored；`cargo fmt --check`；`pnpm test:kea` 6 passed（真機）。
- 真機（`10.1.0.2`、Kea 3.2.1；RFC 5737 `192.0.2.0/24`、未用 id 3）：前置掃描現有 id `[1, 2]` 且測試段不存在 → 計畫 `subnet_add` pools 排序正確、gateway `192.0.2.1`、保留差異 1 筆 → 套用 `subnet_added=true`、保留同回合推送、`config_write=ok` → `config-get`／`reservation-get-all` 驗證 CIDR／pools／routers／保留落地 → 重送 `subnet4-add` 遭拒（`Kea 回應錯誤（result=1）：ID of the new IPv4 subnet '3' is already in use`，狀態不變）→ 清理後 `config-get` 僅 id `[1, 2]`、無殘留保留。既有網段與保留全程未動。
