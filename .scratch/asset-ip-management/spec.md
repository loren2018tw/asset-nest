# 規格：資產與 IP 管理（功能階段一）

- 狀態：已定案（2026-10-04，四輪逐題確認），待實作。
- 追加定案（2026-10-04）：資產清單「已指派 IP」欄與搜尋補強、IP 清單「位置」欄與標頭排序、指派對話框資產顯示格式（見票 12–15）。
- 追加定案（2026-10-04，第二批）：位置篩選可輸入過濾；指派對象顯示「描述(廠牌 型號)」；移除 IP 清單 Gateway 欄與排序。資產匯出與網段匯出／匯入見 `.scratch/csv-export-import/spec.md`。
- 追加定案（2026-10-05）：兩張清單標頭排序修正（第二次點擊反向、箭頭跟隨所點欄位；同欄只在 asc／desc 切換）；資產清單「已指派 IP」改為可排序（見票 19）。
- 詞彙依 `GLOSSARY.md`；關鍵取捨見 `docs/adr/0005`（指派以 Interface 為對象）與 `docs/adr/0006`（兩層驗證）。

## 1. 範圍

### 1.1 本階段包含

- 資產管理：清單、搜尋、篩選、新增/編輯對話框、介面管理、刪除。
- 網段設定：IPv4/IPv6 網段 CRUD（pool、gateway、Kea subnet-id 僅 v4）。
- IP 管理：v4 全枚舉、v6 登錄制；指派到介面（手動／保留）、取消指派、衝突標記。
- 位置輸入建議、屆齡提示。

### 1.2 本階段不含（後續階段）

- Kea 推送、對帳、租約（動態）顯示。
- DHCPv6：v6 無 pool、無 Reservation；v6 位址用途固定「手動設定」。
- 匯入（Excel/CSV）。
- 登入（維持 no-op，後續比照 Kealight 簡易密碼）。
- 指派歷程（僅記目前指派）。
- 共享／虛擬位址（同一 IP 指派給多台設備）。

## 2. 領域模型

### 2.1 Asset（資產）

| 欄位 | 必填 | 說明 |
|---|---|---|
| id | — | 資料庫自增，不出現在 UI |
| property_no 財產編號 | 否 | |
| description 描述 | 是 | |
| location 位置 | 是 | 自由文字；輸入時以既有值建議（`GET /locations`） |
| device_serial 設備序號 | 否 | 可搜尋；重複僅提示 |
| brand 廠牌 | 否 | |
| model 型號 | 否 | |
| purchase_date 購置日期 | 否 | 日期 |
| lifespan_years 年限 | 否 | 整數年 |
| note 備註 | 否 | |
| tags 標籤 | 否 | 多值自由文字（JSON 陣列、預設 `[]`）；輸入時以既有值建議（`GET /tags`）；正規化為 trim、忽略空字串、不分大小寫去重 |
| created_at / updated_at | — | |

- **屆齡徽章**：`purchase_date + lifespan_years < 今天` 時顯示（僅提示）。
- **列表預設欄位**：財產編號、描述、位置、已指派 IP、廠牌、型號、備註、標籤、屆齡徽章。
- **已指派 IP 欄**：列出該資產全部已指派位址（跨介面、跨網段；v4 先、v6 後，同地址族依位址數值）；多筆同列並排（chips）、過多換行；未指派顯示「—」；可排序：依第一筆已指派位址（顯示序；見票 19）。
- **搜尋**：單一關鍵字跨 財產編號／描述／設備序號／廠牌／型號／備註／MAC／已指派 IP（大小寫無關、子字串）。**篩選**：位置、廠牌、標籤（不分大小寫完全符合）。皆為伺服器端。列表標題列可點擊快速排序（伺服器端；預設描述升冪，欄位白名單見 §5）。
- **位置篩選輸入**：位置篩選的下拉可輸入文字即時過濾既有位置選項（本地過濾）。
- **刪除**：連動刪除其 Interface、指派與 Reservation；確認對話框顯示「將刪除 N 個介面、M 筆指派（含 K 筆保留）」。

### 2.2 Interface（網路介面）

| 欄位 | 必填 | 說明 |
|---|---|---|
| id | — | |
| asset_id | 是 | 所屬資產 |
| name 名稱 | 條件 | MAC 空白時必填；有 MAC 時選填 |
| mac | 否 | 正規化為小寫冒號格式；全系統重複僅提示 |
| note 備註 | 否 | |

- 由資產編輯對話框管理。
- **刪除**：連動刪除其指派（確認對話框顯示影響的 IP 數）。
- Reservation 需要 MAC；無 MAC 的介面只能有「手動設定」位址。

### 2.3 Subnet（子網段）

| 欄位 | 必填 | 說明 |
|---|---|---|
| cidr | 是 | 單一地址族；全系統唯一、不得重疊（含嵌套） |
| name 名稱 | 否 | 不強制唯一（雙棧同名可） |
| note 備註 | 否 | |
| gateway | 否 | 須在 CIDR 內；僅標記，該位址仍可被指派 |
| kea_subnet_id | 否 | 僅 v4；唯一；供後續推送 |
| pools | v4 | 多段；每段須在 CIDR 內、彼此不重疊 |

- **列表欄位**：名稱、CIDR、已用/總數、衝突數。「已用」= 手動＋保留筆數；v4「總數」= host 數（扣 network/broadcast；/31、/32 全列）；v6 顯示「已登錄 N」。
- **編輯**：允許縮小 CIDR／擴大 pool；既有指派出界或落池 → 衝突徽章（不阻擋）。
- **刪除**：有任何指派或保留即不可刪（回應顯示數量）；無指派才可刪。

### 2.4 IpAddress / Assignment

- **v4**：IP 清單由 Subnet 範圍推導（伺服器端）；pool 內位址由 pool 範圍推導，不可指派、編輯停用。
- **v6**：登錄制，僅存在已指派位址；可由 IP 頁「新增位址」。
- 用途：**手動設定（static）**／**DHCPv4 保留（reservation）**；未指派即「可用」。v6 恆為手動。
- 指派對象是 Interface（ADR-0005）；同一 Interface 在同一 Subnet 至多一個位址；跨 Subnet（含 v4+v6 並存）可各一。
- 只記目前指派，無歷程；取消指派即回「可用」。IP 值不可修改（改派＝取消＋對其他位址指派）。

### 2.5 資料表草稿

```sql
assets(id, property_no, description NOT NULL, location NOT NULL,
       device_serial, brand, model, purchase_date, lifespan_years, note,
       tags NOT NULL DEFAULT '[]', created_at, updated_at)

interfaces(id, asset_id NOT NULL REFERENCES assets ON DELETE CASCADE,
           name, mac, note, created_at, updated_at,
           CHECK (mac IS NOT NULL OR (name IS NOT NULL AND name <> '')))

subnets(id, cidr NOT NULL UNIQUE, name, note, gateway,
        kea_subnet_id UNIQUE, created_at, updated_at)

subnet_pools(id, subnet_id NOT NULL REFERENCES subnets ON DELETE CASCADE,
             start_ip NOT NULL, end_ip NOT NULL)

ip_assignments(id, subnet_id NOT NULL REFERENCES subnets,
               address NOT NULL,
               interface_id NOT NULL REFERENCES interfaces ON DELETE CASCADE,
               purpose NOT NULL CHECK (purpose IN ('static','reservation')),
               hostname, created_at, updated_at,
               UNIQUE (subnet_id, address),
               UNIQUE (interface_id, subnet_id))
```

網段重疊、pool 合法性、Reservation 需 MAC、網段非空不可刪等結構規則於應用層檢查。

## 3. 驗證規則（兩層，見 ADR-0006）

### 3.1 結構：阻擋儲存

- CIDR 格式合法；gateway 須在 CIDR 內。
- pool 每段須在 CIDR 內、彼此不重疊。
- 網段不得與既有網段重疊（含完全相同、嵌套）。
- Reservation 需介面有 MAC。
- 同一 Subnet 同一位址不得重複指派（UNIQUE）。
- 非空（有指派或保留）的網段不可刪除。
- MAC 空白的介面必須有名稱。

### 3.2 語意：標記提示，不阻擋

- **DuplicateHwAddress**：同一 Subnet 同 MAC 出現多筆保留。
- **IpInPool**：指派位址落在 pool 內。
- **IpOutOfSubnet**：指派位址不在 CIDR 內。
- **IpInUse**：由結構規則涵蓋（同網段不重複指派）；詞彙保留供對帳階段與 Kea 比對。

顯示位置：IP 列徽章、儲存時警示訊息、網段列表衝突計數。

## 4. 操作界面

### 4.1 資產管理

- 清單（§2.1 欄位）＋搜尋/篩選；列操作：編輯、刪除。
- 新增/編輯對話框：欄位＋位置 autocomplete；**介面子編輯器**（新增/刪除、名稱/MAC/備註）；已指派 IP 唯讀顯示（指派一律在 IP 管理頁操作）。
- 刪除確認顯示連動數量。

### 4.2 網段設定

- 清單（名稱、CIDR、已用/總數、衝突數）；新增/編輯（即時顯示重疊等結構錯誤）；刪除（非空擋下）。

### 4.3 IP 管理

- 入口：自網段列表進入（分網段清單）。
- 列：IP、狀態/用途、位置、指派對象、衝突徽章、編輯；未指派列的「位置」顯示「—」。指派對象第一行＝資產描述(廠牌 型號)（廠牌／型號缺者省略，皆缺僅顯示描述），第二行＝介面名稱/MAC（＋hostname）。
- **標頭排序（伺服器端）**：IP、狀態/用途、位置、指派對象可排序；衝突、操作不可。狀態固定序「可用→池內→手動設定→保留」；指派對象依資產描述（不分大小寫）；同鍵以 IP 數值升冪決勝；預設 IP 數值升冪；切換排序回第 1 頁；未指派（空白）固定排最後；v4／v6 一致。
- v4：列出全部 host 位址；pool 列編輯停用。v6：僅登錄位址＋「新增位址」。
- 編輯對話框：搜尋選資產 → 選介面（可當場新增：名稱/MAC）→ 用途（手動/保留；無 MAC 時保留停用；保留可填 hostname）→「取消指派」。已指派列開啟時資產／介面唯讀；換目標＝先「取消指派」再重新指派。
- 對話框的資產顯示一律為「財產編號(描述)」（財產編號為空時僅顯示描述）；適用搜尋選項、選取值、唯讀資產欄、移轉確認訊息與資產端指派對話框副標題。
- IP 值不可改；無刪除按鈕。
- 搜尋/篩選：關鍵字（IP／資產描述／位置／MAC／介面名稱）＋狀態/用途；伺服器端分頁（預設 50 筆）；預設排序 IP 數值升冪（排序見上）。

### 4.4 導覽

- 側欄：資產管理、IP 管理（內含網段設定）。

## 5. API 草稿（`/api/v1`）

| Method | Path | 說明 |
|---|---|---|
| GET | `/assets` | 搜尋/篩選（q／location／brand／device_serial／tag）/排序（sort、dir）/分頁；列含已指派位址（供「已指派 IP」欄；v4 先、v6 後） |
| POST | `/assets` | 新增 |
| GET | `/assets/{id}` | 含 interfaces 與已指派 IP |
| PATCH | `/assets/{id}` | 編輯 |
| DELETE | `/assets/{id}` | 連動刪除 |
| POST | `/assets/{id}/interfaces` | 新增介面 |
| PUT | `/assets/{id}/assignments` | 資產端指派／確認後移轉（見 ADR-0007） |
| PATCH | `/interfaces/{id}` | 編輯介面 |
| DELETE | `/interfaces/{id}` | 刪除介面（連動） |
| GET | `/locations` | 位置建議值（既有值去重） |
| GET | `/tags` | 標籤建議值（既有標籤去重、不分大小寫） |
| GET / POST | `/subnets` | 清單／新增 |
| PATCH / DELETE | `/subnets/{id}` | 編輯／刪除 |
| GET | `/subnets/{id}/ips` | v4 枚舉／v6 登錄；搜尋/篩選/排序/分頁（q 含位置） |
| POST | `/subnets/{id}/ips` | v6 新增登錄位址（含指派） |
| PUT | `/subnets/{id}/ips/{address}/assignment` | 指派／改用途 |
| DELETE | `/subnets/{id}/ips/{address}/assignment` | 取消指派 |

- 沿用既有 JSON 錯誤格式 `{error, message}`；結構錯誤可附 `details`（如衝突網段、受影響筆數）。
- `GET /assets` 排序：`sort` 欄位白名單 `property_no`／`description`／`location`／`brand`／`model`／`note`／`tags`／`expired`／`assigned_ips`；`dir`＝`asc`／`desc`；預設 `description` 升冪；無效值回 400。`assigned_ips` 依第一筆已指派位址（v4 先、v6 後、同族依數值；未指派固定排最後；desc 為完全反向；見票 19）；匯出沿用同一排序。
- `GET /subnets/{id}/ips` 排序：`sort` 欄位白名單 `address`／`status`／`location`／`assignment`；`dir`＝`asc`／`desc`；預設 `address` 升冪；無效值回 400。

## 6. 實作預設（未逐題確認，可直接修改）

- 網段名稱選填。
- 資產列表預設排序：描述（升冪）。
- 分頁預設 50 筆/頁。
- pool 涵蓋 gateway：提示不擋（Kea 動態配發可能把 gateway 派出去；若偏好直接阻擋請改）。
- 位置建議值 = 所有資產位置去重（不分大小寫）。

## 7. 已定案的邊角細節

- `/31`、`/32`：列全部 host；網段可為任意前綴。
- 網段編輯導致出界/落池：標記不擋。
- MAC 重複：提示不擋；MAC 正規化小寫冒號。
- v6 位址用途固定手動；v6 不枚舉空閒位址。
- gateway 可指派（僅標記）。
- 網段名稱可重複。
- 屆齡僅提示、不影響操作。
- IP 頁無刪除按鈕；池內列的編輯停用。
- IP 清單依「位置」或「指派對象」排序時，未指派（空白）固定排最後。
- 編輯既有指派不開放直接更換資產／介面；換目標＝先取消指派再重新指派。
- v4 非 IP 欄排序採掃描後排序，成本與關鍵字搜尋同級；不設位址數上限。
- 指派對話框的資產顯示格式為「財產編號(描述)」，無財產編號時僅顯示描述。

## 8. 決策出處

- `GLOSSARY.md`：Asset / Interface / Subnet / IpAddress / Assignment / Reservation / pool 等詞彙與狀態定義。
- `docs/adr/0005`：指派以 Interface 為對象、同介面同網段至多一位址、無 MAC 不可保留、僅記目前指派。
- `docs/adr/0006`：結構錯誤阻擋、語意衝突標記的兩層驗證原則。
- `docs/adr/0001`~`0004`：Kealight 獨立、單向推送、SQLite、同源部署。
