# spec — 排除範圍（subnet exclusions）

- 狀態：已定案（經 2026-10-06 grilling 逐題確認）
- 決策：`docs/adr/0020-subnet-exclusions.md`
- 詞彙：`GLOSSARY.md`（「排除範圍」「排除」「IpInExcludedRange」已登錄）

## 1. 背景與目標

現行「不可指派」只有 DHCP 位址池（`subnet_pools`）一種來源。實務上網段內有部分位址供其他機制使用（例如 NAT 對外位址、特殊設備），不可指派給資產的 Interface；把它們塞進 pool 語意不符，且 `available` 會誤列為可用。

目標：

1. 網段設定新增 pool 式的「排除範圍」清單（起訖＋選填用途說明）。
2. IP 清單新增「排除」狀態；排除範圍涵蓋既有指派時標記 `IpInExcludedRange`。
3. 觀測不再把排除範圍內的位址標為「非法佔用 IP」。
4. 指派 IP 輸入框：輸入部分 IP（前綴）即時篩選**可用**位址；輸入完整但不可用時顯示原因。

## 2. 名詞與不變量

- **排除範圍**：Subnet 內不可指派給 Interface 的 v4 位址範圍；可多段；單一位址以起=迄表示；附選填用途說明。
- 不變量：
  - 排除範圍不得與任何 pool 重疊、彼此不得重疊（結構性阻擋，比照 pool 規則；ADR-0006）。
  - 排除範圍內的位址不可「新指派」；既有指派不強制解除，以語意衝突標記（兩層分界）。
  - 排除範圍不與 Kea 互動：不推送、不入同步計畫。
- IPv6 不支援排除範圍（與 pool 相同限制）。

## 3. 資料模型

新增 `backend/migrations/0010_subnet_exclusions.sql`：

```sql
CREATE TABLE IF NOT EXISTS subnet_exclusions (
    id        INTEGER PRIMARY KEY AUTOINCREMENT,
    subnet_id INTEGER NOT NULL REFERENCES subnets(id) ON DELETE CASCADE,
    start_ip  TEXT NOT NULL,
    end_ip    TEXT NOT NULL,
    note      TEXT
);
CREATE INDEX IF NOT EXISTS idx_subnet_exclusions_subnet_id
    ON subnet_exclusions(subnet_id);
```

- `subnets::Subnet` 新增 `exclusions: Vec<Exclusion>`，`Exclusion { id, start_ip, end_ip, note: Option<String> }`；依 `id` 升冪（即輸入順序）。
- `ExclusionInput { start_ip: Option<String>, end_ip: Option<String>, note: Option<String> }`。
- `SubnetInput.exclusions`：`#[serde(default)]`。
- `SubnetPatch.exclusions: Option<Vec<ExclusionInput>>`：`None`＝沿用原值；`Some(...)`＝整批取代（空陣列＝清空）。行為與 `pools` 完全對稱。
- `ValidSubnet.exclusions: Vec<ValidExclusion>`；`ValidExclusion { start: Ipv4Addr, end: Ipv4Addr, note: Option<String> }`。
- 寫入：`create`／`update` 與 pools 同交易；`update` 先 `DELETE FROM subnet_exclusions WHERE subnet_id = ?` 再逐列插入。
- 讀取：`get`、`list`、`list_full`、`find_by_address` 皆載入排除範圍（`Subnet` 一致）。
- 刪除網段連動刪除（FK CASCADE）。

## 4. 結構驗證（`subnets::validate_fields`）

| # | 規則 | 錯誤欄位 | 訊息 |
|---|------|----------|------|
| 1 | v6 網段帶非空排除範圍 | `exclusions` | 「IPv6 網段不支援排除範圍」 |
| 2 | 端點必填、須為 IPv4 | `exclusions[i].start_ip`／`end_ip` | 「排除範圍位址格式錯誤：{text}」 |
| 3 | 起點 ≤ 終點 | `exclusions[i].start_ip` | 「exclusions[i] 起點 {start} 不可大於終點 {end}」 |
| 4 | 兩端點在 CIDR 內 | `exclusions[i].start_ip` | 「exclusions[i] 範圍 {start}–{end} 不在網段 {cidr} 內」 |
| 5 | 排除範圍彼此不重疊（端點皆含） | `exclusions` | 「exclusions[i]（a–b）與 exclusions[j]（c–d）重疊」 |
| 6 | 排除範圍不得與 pool 重疊 | `exclusions` | 「exclusions[i]（a–b）與 pools[j]（c–d）重疊（排除範圍不得與 DHCP 位址池重疊）」 |
| 7 | 用途說明 trim 後不得含 `|`（CSV 分隔符） | `exclusions[i].note` | 「用途說明不可包含 |」 |

`#` 可出現於用途說明（CSV 以**第一個** `#` 分隔範圍與說明）。

## 5. IpAddress 狀態、排序與清單

- `IpStatusFilter` 新增 `Excluded`，查詢字串 `excluded`；`GET /subnets/{id}/ips?status=excluded` 可用。
- 狀態判定（v4）：有指派 → `static`／`reservation`；否則在 pool → `in_pool`；否則在排除範圍 → `excluded`；否則 `available`。
- `available` 定義更新：未指派、無保留、未被租用、不在 pool、**不在排除範圍**。
- 狀態排序：`available(0) → in_pool(1) → excluded(2) → static(3) → reservation(4)`。
- 觀測篩選 `unassigned_seen`（非法佔用 IP）：位址在排除範圍內時不列入（與 pool 同處理）。
- 白名單錯誤訊息：狀態篩選須為 `available、in_pool、excluded、static 或 reservation`。
- `IpEntry` 欄位不變（`status` 即足以辨識排除）。

## 6. 語意衝突（conflicts）

- 新代碼 `IpInExcludedRange`（v4）：已指派的位址落在排除範圍內。
- `detect` 代碼順序：`IpOutOfSubnet → IpInPool → IpInExcludedRange → DuplicateHwAddress`（pool 與排除範圍結構互斥，不會同時命中）。
- `warnings_for` 訊息：「位址 {address} 落在排除範圍內（僅提示，不阻擋儲存）」。
- `observed_codes`：`ObservedOnUnassigned` 僅在「未指派、非池內、**非排除範圍**」且 `last_seen_mac` 非空時標記。
  - 理由：NAT 等位址本來就會被 ARP 觀測到；其 MAC 可能是實體介面、VRRP/HSRP 虛擬 MAC，或 HA 切換／虛擬化而變動，不可依賴。
- 網段衝突數（`SubnetSummary.conflicts`）計入 `IpInExcludedRange`（走 `detect` 即生效）。

## 7. 指派驗證（assignments）

- `validate_address`（`existing == false`）v4 依序：host 範圍檢查 → pool 檢查 → **排除範圍檢查**：
  - 錯誤：「位址 {address} 落在排除範圍內，不可指派」（400、field `address`）。
- `existing == true`（同介面同位址更新、既有位址移轉）不重驗，與 pool 同原則（ADR-0006）：涵蓋後加的排除範圍以 `IpInExcludedRange` 標記呈現。
- 影響範圍：`assign`、`assign_for_asset`（含移轉到未指派位址）、`PUT /subnets/{id}/ips/{address}/assignment`、`PUT /assets/{id}/assignments`。
- v6 登錄不受影響。

## 8. 指派候選 API

`GET /api/v1/ip-candidates?q=<prefix>&limit=20`

- `limit`：預設 20、夾在 `1..=50`。
- **前綴語意（octet 邊界）**：
  - `q` 以 `.` 分段；結尾帶點＝所有段皆為完整 octet（精確）；否則**最後一段一律為「前綴段」（四段亦然）**。
  - 完整 octet 精確比對；前綴段比對該 octet 十進位字串的前綴（`1` → 1、10–19、100–199；`0` → 0）。
  - 範例：`10.0.1` 命中第三段 ∈ {1, 10–19, 100–199}；`10.0.1.` 命中 `10.0.1.0/24`；`10.1.1.6` 命中 `10.1.1.6`、`10.1.1.60–69`；`10.1.1.6.` 精確單一位址。
  - 至少 **2 個完整 octet**，否則 400 `invalid_query`（field `q`）：「查詢前綴至少須包含兩個完整 octet（例：10.0.）」。
  - 格式錯誤（非數字、超過 4 段、空段、單段超過 3 位數、完整 octet > 255）→ 400 `invalid_query`。
- **取樣順序（見票 10）**：含前綴段（多值）時以值輪流取樣——每個前綴段值先取一筆（升冪、各取該值之下一個可用位址），再回到首值取下一筆，直到 `limit`；確保 20 筆視窗內每個前綴段值都出現（`10.1.6` → `10.1.6.0`、`10.1.60.0`、…、`10.1.69.0`、`10.1.6.4`、…）。無前綴段（結尾點）維持位址升冪。
- **回應**：

```json
{
  "items": [
    { "address": "10.0.1.5", "subnet_id": 3,
      "subnet_cidr": "10.0.1.0/24", "subnet_name": "辦公室" }
  ],
  "query_status": {
    "address": "10.0.1.5", "status": "excluded",
    "subnet_id": 3, "subnet_cidr": "10.0.1.0/24", "subnet_name": "辦公室"
  }
}
```

- `items`：僅 v4 **host** 且「可用」（無指派、非池內、非排除範圍）；排序：網段 CIDR 網路位址升冪、位址升冪；取至 `limit`（不提供 total）。無結果＝空陣列。
- `query_status`：僅當 `q` 為完整 v4 位址時出現（否則 `null`）。`status ∈ available | in_pool | excluded | static | reservation | out_of_subnet`。
  - 判定順序：無所屬網段 → `out_of_subnet`（subnet 欄位 null）；有指派 → 該用途；非 host（network/broadcast）→ `out_of_subnet`；池內 → `in_pool`；排除範圍內 → `excluded`；否則 `available`。
  - `subnet_id`／`subnet_cidr`／`subnet_name`：位址落在某網段 CIDR 內時帶值；完全找不到網段時為 null。
- **效能界線**：至少 2 個完整 octet ⇒ 匹配空間 ≤ /16（65,536 個位址）。實作以 host 範圍與前綴交集列舉、達 `limit` 即停止；不得做無界掃描。

## 9. CSV 匯出／匯入（ADR-0009 擴充）

- 匯出改為 **7 欄**：`名稱, CIDR, Gateway, Kea subnet-id, 位址池, 排除範圍, 備註`。
- 「排除範圍」儲存格：多段以 `|` 分隔；每段 `起-迄`；有用途說明時 `起-迄#用途說明`（以第一個 `#` 分隔）。v6 一律空字串。
- 匯入：新增**選填**標題「排除範圍」；欄位缺席視為未填（舊檔相容）。
  - v6 帶內容 → 錯誤碼 `exclusions_for_v6`（field `exclusions`）。
  - 段格式錯誤 → `invalid_exclusions`。
  - 其餘重用 `SubnetInput::validate`；`validate_issue` 將 `exclusions` 或 `exclusions[...]` 映為 `invalid_exclusions`。
- `RowData` 新增 `exclusions: Vec<String>`（正規化：`起-迄` 或 `起-迄#note`）。`column_count_mismatch` 的 `mismatch_data` 一併填入。
- 欄位順序與 `Field` enum：`Pools` 與 `Note` 之間插入 `Exclusions`；`FIELD_COUNT` 6 → 7。
- 問題列報告（前端）欄位同步插入「排除範圍」。

## 10. 前端行為

### 10.1 網段表單（`SubnetFormDialog.vue`）

- v4 顯示「排除範圍」區塊（pool 區塊之後）：每列起點／終點／用途說明（選填）＋刪除鈕、新增鈕。
- 區塊提示：「排除範圍內位址不可指派（例：NAT 對外）；不得與 pool 或彼此重疊」。
- 即時檢查（mirror 後端，後端仍權威）：僅 IPv4、端點必填、起點 ≤ 終點、端點在 CIDR 內、彼此不重疊、與 pool 不重疊、用途說明不含 `|`；問題欄位 `exclusions`（共用錯誤 banner 與「請修正後再儲存」流程）。
- 載入既有值（含 note）與送出（v4 帶 `exclusions`、v6 帶空陣列）。

### 10.2 IP 清單（`IpListPage.vue`）

- `IpStatus` 新增 `"excluded"`；v4 篩選選項新增「排除」。
- 狀態標籤「排除」、徽章色 `orange`。
- 操作欄：`status === "excluded" && assignment === null` → 編輯鈕停用，tooltip「排除範圍內位址不可指派」。
- 衝突徽章：`IpInExcludedRange`＝標籤「排除範圍」／說明「指派的位址落在排除範圍內（僅提示，不阻擋）」。
- `ObservedOnUnassigned` 說明文字補上「非排除範圍」。

### 10.3 指派對話框（`AssignIpDialog.vue`）

- 位址欄由 `q-input` 改為 `q-select`（`use-input`、`hide-selected`、`fill-input`、`input-debounce="300"`），**仍接受自由輸入**（移轉既有指派時該位址不會出現在候選中）。
- 查詢時機：輸入達 2 個完整 v4 octet；由新 helper `parseIpv4Prefix()`（`utils/cidr.ts`）判定，語意與後端 §8 相同。
- 選項標籤：`10.0.1.5`＋次要文字「網段名｜CIDR」；上限 20 筆。
- 查無結果：顯示提示「此範圍無可用位址」（不擋送出）。
- `query_status` 即時提示（`q` 為完整位址時）：
  - `in_pool` →「此位址在 DHCP 位址池內，不可指派」（負向）
  - `excluded` →「此位址在排除範圍內，不可指派」（負向）
  - `static`／`reservation` →「此位址已指派；送出後可確認移轉」（中性）
  - `out_of_subnet` →「此位址不在任何網段可指派的範圍內」（負向）
  - `available` → 不顯示
- 候選請求失敗不得阻擋輸入與送出；過期回應以 token 丟棄。
- v6 不查詢候選（登錄制）。
- 新 API 封裝：`frontend/src/api/ipCandidates.ts`。

### 10.4 匯入對話框與問題列報告

- `SubnetImportDialog.vue` 預覽表新增「排除範圍」欄（以 `|` 串接顯示）。
- `utils/subnetImport.ts` 問題列報告欄位插入「排除範圍」。

## 11. Kea 同步

- 不變更：排除範圍不出現在 `GET /kea/sync/plan`、不推送、不影響 gateway／pool 對齊（ADR-0011／0013）。
- 既有 Reservation 被排除範圍覆蓋時，因 `IpInExcludedRange` 屬語意衝突，沿用既有機制於完整同步標為「跳過推送」（不新增、不刪除 Kea 既有保留）；pool 與排除範圍互斥，Kea 仍不可能動態配發該位址。
- 因結構互斥（§4 #6），Kea 動態配發不可能落在排除位址。

## 12. 非目標

- Kea 端的排除範圍原語或同步（Kea 無此概念）。
- IPv6 排除範圍。
- 排除位址與「未知裝置」清單的關聯（MAC 面向行為不變）。
- 排除位址的預期 MAC／預期設備結構化登錄（用途說明可自由記載）。

## 13. 測試矩陣

| 模組 | 測試重點 |
|------|----------|
| migration／delete_linkage | 刪除網段連動刪除 `subnet_exclusions` |
| subnets 單元 | §4 每條規則；`#` 允許、`|` 拒絕；PATCH 合併語意 |
| tests/subnets | CRUD 往返、取代／清空、v6 拒絕、與 pool 重疊 400 |
| ips 單元 | `excluded` 狀態、篩選、排序 rank、`unassigned_seen` 排除 |
| tests/ips | `?status=excluded`、available 不含排除、已指派＋後加排除 → 徽章、觀測抑制 |
| conflicts 單元 | `IpInExcludedRange` 偵測與訊息、代碼順序、`ObservedOnUnassigned` 抑制 |
| tests/assignments | 新指派進排除 → 400（含資產端、移轉到未指派位址） |
| tests/ip_candidates | 前綴語意（`10.0.1.` 不中 `10.0.10.x`；`10.0.1` 依前綴命中第三段 1、10–19、100–199；`10.1.1.6` 命中 `.6` 與 `.60–69`；`10.1.1.6.` 精確）、值輪流取樣、可用性過濾、limit、`query_status` 六態、400 邊界 |
| tests/subnet_import | 7 欄往返（含 `#note`）、v6 拒絕、格式錯誤碼、舊 6 欄檔相容 |
| tests/kea_sync | 回歸：排除範圍不影響計畫（無需新行為） |
| frontend | `pnpm --filter frontend typecheck`＋`pnpm lint:check`；人工檢核清單見各票 |

## 14. 實作票

| # | 票 | 依賴 |
|---|----|------|
| 01 | 排除範圍資料模型與網段 CRUD | — |
| 02 | IP 狀態「排除」、篩選與衝突標記 | 01 |
| 03 | 指派阻擋（新指派進排除範圍） | 01 |
| 04 | 指派候選端點 `GET /api/v1/ip-candidates` | 01 |
| 05 | 網段 CSV 匯出／匯入（排除範圍欄） | 01 |
| 06 | 前端：網段表單與匯入呈現 | 01、05 |
| 07 | 前端：IP 清單狀態與篩選 | 02 |
| 08 | 前端：指派輸入候選與提示 | 04 |
| 09 | 整合驗證（全測試與檢核） | 全部 |
