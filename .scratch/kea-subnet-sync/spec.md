# Kea 網段層同步：位址池與 gateway

對應票號：01（後端差異與套用）、02（前端對話框）、03（deploy 載入 subnet_cmds hook＋文件）。

決策見 `docs/adr/0013`；延伸 `docs/adr/0011`（保留同步）與 `docs/adr/0012`（一鍵安裝）。

## 目標

- 「Kea 同步」按鈕（完整同步）除保留外，一併對齊受管網段的**網段層設定**：
  - **DHCP 位址池（pool）**：asset-nest `subnet_pools` ↔ Kea `subnet4[].pools`。
  - **gateway**：asset-nest `subnet.gateway` ↔ Kea `subnet4[].option-data` 的 `routers` 選項。
- 先 dry-run 計畫、確認後套用；差異在對話框中可見。

## 範圍

- 僅受管網段（v4＋`kea_subnet_id`）；v6 與未受管網段完全不碰。
- **網段本身不新增／刪除**：受管網段必須已存在於 Kea，且 `subnet-id` 與 CIDR 相符才同步；
  不符者整段跳過並回報（沿用 ADR-0011）。
- **嚴格對齊**（以 asset-nest 為準）：
  - pool：Kea 端多出的 pool 刪除；asset-nest 空 pool＝該網段無動態配發。
  - gateway：Kea 端 `routers` 與 asset-nest `gateway` 不符即改；`gateway` 未設＝移除 `routers`。
- **只在完整同步推送**：編輯網段（PATCH）不即時推送；指派即時推送行為不變（僅保留）。
- 不做 Kea 端 pool 的反向吸收、不做雙向同步。

## 後端

### client（`kea::http::Client`）

- `config_get_dhcp4()` 的 `subnets` 改為 `subnet-id → KeaSubnet`：
  - `cidr`、`pools`（正規化範圍）、`gateway`（`routers` option 值；無則 `None`）、
    `raw`（原始 `subnet4` 物件，供整段回寫）。
  - pool 正規化：範圍字串 `a - b` 與 CIDR 形式皆轉為 `start-end`（CIDR 以整段展開；
    Kea v4 prefix pool 可配發 network／broadcast，見 ARM）。v6 不涉。
  - `routers` 條目判定：`option-data` 中 `name == "routers"`（或 `code == 3` 且
    `space == "dhcp4"`）的第一筆；含 `client-classes` 者不特別處理（限制見下）。
- `subnet4_update(subnet: &Value)`：`subnet4-update`（subnet_cmds hook；
  `arguments.subnet4 = [整段物件]`）。修改命令；成功後由完整同步統一 `config-write`。
- `kea_subnets()` 行為不變（`subnet-id → CIDR`，供既有真機測試）。

### `kea::sync`

- `compute_plans`：以 `config_get_dhcp4()` 取得 KeaSubnet；CIDR／subnet-id 檢查不變。
  在既有保留差異外，計算：
  - `pool_add`／`pool_delete`：以正規化範圍比較、數值排序；同範圍但 Kea 端帶額外屬性
    視為相同（不重建）。
  - `gateway`：`{ current, desired }`，兩者不同才出現（含 `null`）。
- 計畫（`GET /api/v1/kea/sync/plan`）：每受管網段新增

```json
{
  "pool_add": ["140.128.179.126-140.128.179.138"],
  "pool_delete": [],
  "gateway": { "current": "140.128.179.254", "desired": "10.1.1.1" }
}
```

  `totals` 新增 `pool_add`／`pool_delete`／`gateway`（各為筆數；gateway 每網段至多 1）。
  有 `error` 的網段不計算網段層差異。
- 套用（`POST /api/v1/kea/sync`）：保留三相位（刪→改→增）後，逐受管網段（序列）執行：
  - 由 `raw` 複製整段物件，`pools` 重建成期望集合：同範圍的既有條目原樣保留
    （保留 `client-classes` 等屬性）、缺少的以 `{"pool": "start - end"}` 新增、
    多餘的移除。
  - `option-data` 的 `routers` 條目改值／新增（`{"name":"routers","code":3,"space":"dhcp4","data":...}`）／移除。
  - 呼叫 `subnet4_update`；成功才計入 `pool_added`／`pool_deleted`／`gateway_updated`。
    失敗記 `settings_error`（不阻擋其他網段）。
- `config-write`：保留或網段層任一成功變更才呼叫一次（維持既有語意）。
- 報告：每網段新增 `pool_added`、`pool_deleted`、`gateway_updated`（bool）、
  `settings_error`（選填）；`failures` 仍為保留項。

### 限制（已知並記錄）

- Kea 端 pool 若帶 `client-classes` 且需要變更（範圍不同），該條目會以純範圍重建，
  屬性遺失；同範圍不重建。
- `subnet4-update` 需載入 `subnet_cmds` hook；未載入時計畫照常顯示差異、套用時
  該網段記 `settings_error`（訊息含 `subnet4-update`）。deploy 已載入（票 03）。

## 前端

- `api/kea.ts`：`KeaPlanSubnet` 增 `pool_add`／`pool_delete`／`gateway`；
  `KeaSyncPlan.totals` 增 `pool_add`／`pool_delete`／`gateway`；
  `KeaApplySubnet` 增 `pool_added`／`pool_deleted`／`gateway_updated`／`settings_error`。
- `KeaSyncDialog.vue`：
  - 計畫 banner 與每網段摘要含位址池與 gateway 變更；明細列出新增／刪除的 pool
    範圍與 gateway 變更（`current → desired`；`null` 顯示「（未設）」／「（移除）」）。
  - 「套用同步」按鈕在僅有網段層差異時也可按。
  - 結果 banner 與每網段列含位址池／gateway 計數；`settings_error` 以 negative 顯示，
    `$q.notify` 視為部分失敗。

## 測試

- 單元（`kea::sync`／`kea::http`）：pool 字串與 CIDR 正規化、pool diff、gateway 抽取。
- 整合（stub Kea，沿用 `tests/kea_sync.rs` 模式）：
  - 計畫：pool 新增／刪除與 gateway 變更（含 gateway 移除、`null` 顯示）。
  - 套用：呼叫一次 `subnet4-update`、整段其他欄位與其他 option-data 原樣保留、
    同範圍 pool 屬性保留、多餘 pool 刪除、gateway 改值／移除。
  - 無網段層差異：不呼叫 `subnet4-update`、不 `config-write`。
  - `subnet4-update` 失敗：記 `settings_error`、不阻擋保留推送、無成功變更時不寫檔。
  - 與保留混合變更：整批仍只 `config-write` 一次。
- 真機（`pnpm test:kea`，`#[ignore]`）：唯讀 `config-get` 解析 pool／gateway；
  `list-commands` 檢查 `subnet4-update`（未載入 hook 時照實記錄）；不送修改命令。
- 前端：無測試基礎設施，不新增（typecheck 與 lint 把關）。

## 指令

```sh
cargo test --manifest-path backend/Cargo.toml   # 含 stub 整合測試
pnpm test:kea                                    # 真機（需 .env）
pnpm lint:check                                  # 前端
```

## 實作記錄

- 票 01（後端）：主實作 commit `b05d616`（client `KeaSubnet`／`Ipv4Range`／`subnet4_update`／`list_commands`；sync 計畫與套用加入 pool／gateway、`settings_error`、單次 `config-write`；stub＋真機唯讀測試）。
- 票 02（前端）：主實作 commit `169fc63`（`api/kea.ts` 型別擴充、`KeaSyncDialog.vue` 計數／明細／錯誤呈現、僅網段層差異可套用）。
- 票 03（deploy＋文件）：主實作 commit `6903f82`（`configure_kea`／`render_kea_config` 加入 subnet_cmds、既有設定 warn；ADR-0013、GLOSSARY、README、`.scratch/kea-sync/spec.md` 指標）。
- 驗收：`cargo test --manifest-path backend/Cargo.toml`（104 單元＋137 整合、共 241 passed、5 ignored、0 failed）；`cargo fmt --check`；`pnpm lint:check`；`pnpm --filter frontend typecheck`；`pnpm build:frontend` 全綠。
- 真機（`10.1.0.2`）：`subnet_cmds` 尚未載入（`list-commands` 無 `subnet4-update`）；載入後跑 `pnpm test:kea` 重驗（見票 03 Comments）。
