# Kea 檢視：租約清單與系統狀態

對應票號：01（後端狀態端點）、02（後端租約端點）、03（前端 Kea 區段）。

## 目標

- 側邊欄新增「Kea」區段，提供兩個唯讀頁面：
  - **租約清單**：即時檢視 Kea DHCPv4 動態配發結果（不必登入 Kea 主機）。
  - **系統狀態**：版本、監聽介面、運行資訊、DHCPv4 摘要與連線狀態；開發／安裝預設「不監聽」一眼可見。
- 兩個頁面皆唯讀、不落庫；Lease 真實來源為 Kea（見 GLOSSARY；ADR-0002）。

## 範圍

- 僅 IPv4 租約（`lease4-get-all`）。
- 不做租約對帳（與本地保留／指派比對）、不做租約變更（釋放／刪除）、不做 IPv6、不改動 `deploy/`。
- 不改動既有 Kea 同步行為、端點與對話框。

## 領域詞彙

- 新增 GLOSSARY 詞條「**監聽介面（listening interface）**」：Kea 伺服器上 DHCP 服務綁定的作業系統網路介面（如 `eth0`）；與 Asset 的 Interface 不同；空清單＝不主動服務 DHCP（安裝預設，見 ADR-0012）。
- 統一用「介面」而非「界面」。

## 後端

### client（`kea::http::Client`）

- 沿用 `version_get`；新增：
  - `config_get_dhcp4()` → `{ interfaces: Vec<String>, lease_backend: Option<String>, subnets: HashMap<i64, String> }`；`kea_subnets()` 改為重用（行為不變）。
  - `status_get()` → `{ pid, uptime, reload: Option<i64>, sockets: Option<SocketStatus> }`（`SocketStatus { status }`）；缺欄位為 `None`。真機 3.2.1 實測：`uptime`／`reload` 為相對秒數、`sockets` 為狀態物件（如 `{"status":"ready"}`）而非綁定清單。
  - `lease4_get_all()` → `Vec<KeaLease>`：`ip_address`、`hw_address`、`hostname`、`subnet_id`、`cltt`、`valid_lft`、`state`（數字 0／1／2／3 → `default`／`declined`／`expired`／`released`；文字原樣小寫；未知保留原值）；`result` 3（空）視為空清單。
  - `base_url()`：顯示用連線目標（去 userinfo、去尾斜線）。
- 僅讀取命令，不帶 `operation-target`、不呼叫 `config-write`。

### `GET /api/v1/kea/leases`

- 未設定 Kea → 400 `validation_error`（訊息含 `KEA_API_URL`，沿用現行風格）。
- 命令失敗 → 502 `kea_error`。
- 200 回應：

```json
{
  "leases": [
    {
      "ip_address": "10.0.0.5",
      "hw_address": "aa:bb:cc:dd:ee:ff",
      "hostname": "pc-01",
      "subnet_id": 1,
      "subnet_cidr": "10.0.0.0/24",
      "subnet_name": null,
      "expires_at": "2026-10-05T12:00:00Z",
      "state": "default"
    }
  ]
}
```

- `expires_at = cltt + valid_lft`（ISO 8601 UTC；缺欄位為 `null`）。
- `subnet_cidr`／`subnet_name`：以本地 `subnets.kea_subnet_id = subnet_id` 對應（受管網段）；無對應為 `null`。

### `GET /api/v1/kea/status`

- **一律 200**；失敗以旗標與分區錯誤呈現（診斷頁）。三命令各自獨立嘗試（並行 `join!` 縮短最壞延遲）：

```json
{
  "configured": true,
  "reachable": true,
  "url": "http://127.0.0.1:8000",
  "version": { "version": "3.2.1", "text": "..." },
  "interfaces": ["eth0"],
  "runtime": { "pid": 123, "uptime": 456, "reload": 789, "sockets": { "status": "ready" } },
  "dhcp4": { "subnet_count": 1, "managed_subnet_count": 1, "lease_backend": "memfile" },
  "errors": { "config": "..." }
}
```

- `configured=false`（未設 `KEA_API_URL`）→ `reachable=false`、其餘欄位 `null`、不發任何命令。
- `reachable` 以 `version-get` 成功與否判定；成功區塊照常回傳，失敗區塊為 `null` 並在 `errors` 記錄（key：`version`／`config`／`status`）。
- `interfaces`＝`Dhcp4.interfaces-config.interfaces`；**空陣列＝未監聽**（中性呈現，非錯誤）。
- `runtime.sockets`：`status-get` 有提供就帶出（真機 3.2.1 實測為物件 `{"status":"ready"}`），否則 `null`。
- `dhcp4.subnet_count`：`Dhcp4.subnet4` 筆數；`managed_subnet_count`：本地 `kea_subnet_id IS NOT NULL` 的網段數。

## 前端

- 側邊欄（`MainLayout.vue`）：`q-item-label` 標題「Kea」；項目「租約清單」（icon `receipt_long`，`/kea/leases`）、「系統狀態」（icon `monitor_heart`，`/kea/status`）；`/kea` 前綴 active 高亮。
- 路由（`routes.ts`）：`kea/leases` → `KeaLeasesPage.vue`、`kea/status` → `KeaStatusPage.vue`（lazy import）。
- `api/kea.ts` 新增 `getKeaStatus()`、`listKeaLeases()` 與型別。
- **租約清單頁**：
  - `q-table`（client-side）欄位：IP、MAC、Hostname、網段（CIDR（名稱）；無對應顯示 `Kea #id`）、到期時間（本地時區；已到期淡化）、狀態 chip。
  - 狀態標籤：`default`→使用中、`declined`→已拒絕、`expired`→已過期、`released`→已釋放；未知顯示原值；tooltip 顯示正規化後的原始字串。
  - 預設排序 IP 升冪（八位元組數值比較）；每頁 50；搜尋 IP／MAC／hostname（不分大小寫）＋狀態篩選。
  - 進頁自動載入；「重新整理」＋上次更新時間。
  - 錯誤（400／502）：頂部 `q-banner` 顯示訊息＋「前往系統狀態」連結；空表。
- **系統狀態頁**：
  - 區塊：連線（可達／未設定／失敗；顯示 `url`；未設定文案「未設定 Kea 連線（KEA_API_URL）」）、版本、監聽介面（設定值；空＝「未監聽任何介面（不主動服務 DHCP；安裝預設）」；有 runtime sockets 時並列「實際綁定」）、運行資訊（pid／uptime（人化）／reload（本地時間））、DHCPv4 摘要（Kea 網段數 vs 本地受管數；不一致加 warning chip；租約庫類型）。
  - 分區錯誤：各區塊顯示「無法取得」；頁面其餘部分照常運作。
  - 進頁自動載入；「重新整理」＋上次更新時間。

## 測試

- 整合（stub Kea，沿用 `tests/kea_sync.rs` 的 stub 模式）：
  - status：未設定回 200 `configured=false`；stub 正常回各區塊；部分命令失敗時 `reachable`／`errors` 正確且成功區塊仍在。
  - leases：未設定 400；stub 失敗 502；happy path 含 `expires_at` 計算、state 正規化、受管網段 CIDR 對應；0 筆（result 3）回空陣列。
- 真機（`pnpm test:kea`，`#[ignore]`）：唯讀呼叫 `version-get`／`config-get`（interfaces／subnet4）／`status-get`／`lease4-get-all`，實測欄位並定案對應；不 `config-write`、不留變更。
- 前端：無測試基礎設施，不新增（型別檢查與 lint 把關）。

## 指令

```sh
cargo test --manifest-path backend/Cargo.toml   # 含 stub 整合測試
pnpm test:kea                                    # 真機（需 .env）
pnpm lint:check                                  # 前端
```

## 待實測定案（真機）

- `status-get`：已實測（3.2.1）——`pid`／`uptime`／`reload` 為數字，`uptime`／`reload` 為相對秒數（非 epoch 時間）；`sockets` 為物件 `{"status":"ready"}`，非綁定清單；另有 `csv-lease-file`／`dhcp-state`／`thread-pool-size` 等未取用欄位。
- `version-get`：已實測——回 `text: "3.2.1"` 與 `arguments.extended`（完整建置資訊），未提供 `arguments.version`；API 的 `version` 區塊為 `version: null`、`text: "3.2.1"`。
- `config-get`：已實測——`Dhcp4.interfaces-config.interfaces`（可為空陣列＝未監聽）、`Dhcp4.lease-database.type`（`memfile`）、`Dhcp4.subnet4` 皆如預期。
- `lease4-get-all` 的 `state` 型別與 0 筆時 `result`：待票 02 實測。
- 實測結果若與本節牴觸，以實測為準並回頭更新本 spec。

## 實作記錄

- （待實作）
