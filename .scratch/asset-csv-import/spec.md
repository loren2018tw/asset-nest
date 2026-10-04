# 規格：資產 CSV 匯入

- 狀態：已定案（2026-10-04，三輪逐題確認），待實作。
- 詞彙依 `CONTEXT.md`；CSV 欄位格式與寫入語意的權威文件為 `docs/adr/0008`；兩層驗證見 `docs/adr/0006`。

## 1. 範圍

### 1.1 本階段包含

- 資產清單頁「匯入」入口；對話框三步：選檔 → 預覽 → 結果。
- CSV 批次新增資產；每列可附建 `eth0` 介面與 IPv4／IPv6 指派（v4 保留／手動、v6 手動）。
- 範本下載、編碼偵測（UTF-8／Big5）、逐列驗證預覽、問題列報告下載。
- 後端解析與驗證（單一驗證權威）、`dry_run` 預覽、確認後單一交易寫入。

### 1.2 本階段不含

- `.xlsx` 直接匯入。
- 更新既有資產（upsert）、匯入歷程。
- 一台資產多介面；網段自動建立；IPv6 保留。

## 2. CSV 解析規則

- 欄位總表與值規則：見 ADR-0008；範本下載內容須與 `docs/資產匯入範本.csv` 一致（標題列＋1 範例列，UTF-8 BOM）。
- 標題比對、重複標題、未知標題與缺必要標題的處理：見 ADR-0008；檔級錯誤以 400 `{error, message}` 回報。
- 編碼偵測順序：UTF-8 BOM → 嚴格 UTF-8 → Big5（CP950）；回應附 `encoding`（`utf-8`／`big5`），預覽須顯示。
- UTF-16 或無法解讀 → 400，提示另存為 UTF-8 或 Big5。
- 上限（預設可調）：檔案 5 MB、資料列 5,000 列；超過回 400。
- 列號＝資料記錄序號＋1（標題為第 1 列）；供預覽與問題報告標示。
- 單位格文字一律 trim；空行忽略。

## 3. 寫入語意

- 只新增；任何列不得解讀為更新既有資料。
- 「MAC／IPv4／IPv6／hostname」任一非空 → 建立介面 `eth0`（名稱固定 `eth0`、無備註）；全空 → 不建介面。
- 用途推導：IPv4＋MAC → `reservation`；IPv4 無 MAC → `static`；IPv6 恆 `static`。
- hostname 僅 `reservation` 可填。
- 網段須先建立；由位址反推所屬網段（網段不重疊，至多一個）。
- 全有全無：任一列有結構錯誤 → 整批拒絕；`dry_run=false` 亦不寫入任何資料，回報逐列錯誤。
- 語意警示（重複財產編號、設備序號、MAC；檔內或對既有資料）僅提示，不擋。
- 匯入在單一交易內完成（資產、介面、指派三者原子化）；`dry_run` 預覽後資料若已變動，以正式匯入時的重驗結果為準（可能整批失敗並再次回報錯誤列）。

## 4. 驗證規則

### 4.1 檔級（400，不進入預覽）

- 無法解析、空檔案、無資料列、超過大小或列數、編碼無法解讀。
- 缺「描述」或「位置」標題；標題重複。

### 4.2 列級結構（錯誤；任一列即整批不匯入）

- 描述、位置必填。
- 購置日期無法解析；年限非非負整數。
- MAC 格式錯誤；IPv4／IPv6 格式錯誤或與欄位地址族不符。
- IPv4：不屬任何既有 v4 網段；為 network／broadcast；落在 pool 內；已被指派；同一檔案內重複。
- IPv6：不屬任何既有 v6 網段；已被登錄；同一檔案內重複。
- hostname 有值但該列非「MAC＋IPv4」。

### 4.3 語意警示（不擋）

- `duplicate_property_no`：財產編號命中既有資產或檔內重複。
- `duplicate_device_serial`：設備序號命中既有資產或檔內重複。
- `duplicate_mac`：MAC 命中既有介面或檔內重複。

### 4.4 問題訊息

- 每列問題含 `severity`（`warning`／`error`）、`code`、`field`（可空）、`message`（中文，含數值、網段等具體資訊）。
- 「位址已被指派」的訊息附目前指派對象（資產描述／位置、介面名稱／MAC），比照 ADR-0007 的提示資訊樣式。

## 5. API

### 5.1 端點

`POST /api/v1/assets/import?dry_run=true|false`
- `multipart/form-data`；檔案欄位名 `file`；`dry_run` 預設 `true`。

### 5.2 回應（200）

```json
{
  "dry_run": true,
  "encoding": "utf-8",
  "ignored_headers": ["數量"],
  "summary": { "total": 120, "ok": 110, "warnings": 8, "errors": 2 },
  "rows": [
    {
      "row_number": 2,
      "status": "ok | warning | error",
      "data": { "property_no": "PC-001", "description": "…", "location": "…",
                 "device_serial": null, "brand": null, "model": null,
                 "purchase_date": null, "lifespan_years": null, "note": null,
                 "tags": [], "mac": null, "ipv4": null, "ipv6": null,
                 "hostname": null },
      "issues": [
        { "severity": "warning", "code": "duplicate_device_serial",
          "field": "device_serial", "message": "…" }
      ]
    }
  ],
  "committed": false,
  "created": null
}
```

- `status`：單列取最高嚴重度。
- 正式匯入成功：`dry_run=false`、`committed=true`、`created={assets, interfaces, assignments}`（整數）。
- 正式匯入遇結構錯誤：`committed=false`、`created=null`、`rows` 含錯誤；HTTP 仍為 200（屬驗證報告，非傳輸錯誤）。
- 傳輸層錯誤（缺檔、multipart 格式錯誤、超過上限）走既有 `ApiError` 400。
- 依賴：`axum` multipart feature、`csv`、`encoding_rs`（後端）。

## 6. 操作界面

- 入口：資產清單工具列「匯入」按鈕（icon `upload_file`），位於「新增資產」左側。
- `AssetImportDialog`（`q-dialog`，最大寬約 1400px）三步：
  1. **選檔**：QUploader（關閉自動上傳）接受 `.csv`；說明文字（編碼、標籤以 `|` 分隔、日期格式、需先建網段、全有全無）；「下載範本」；系統尚無網段時顯示警示橫幅。「預覽」→ 以 `dry_run=true` 呼叫。
  2. **預覽**：摘要 chips（總筆數、OK、警示、錯誤、偵測編碼）；忽略欄位提示；表格＝列號＋14 欄＋狀態＋原因，虛擬滾動，提供「只看問題列」切換；有問題列時提供「下載問題列報告」；「匯入 M 筆」（M＝總數－錯誤數）於有任何錯誤時停用，僅有警示時先跳確認。
  3. **結果**：建立數量（資產／介面／指派）、警示數與報告下載、「完成」→ 重載清單與篩選選項。
- 狀態顏色：OK＝綠、警示＝黃、錯誤＝紅（沿用 Quasar badge 語彙）。
- 問題列報告：前端由回應列產生 CSV（UTF-8 BOM）；欄位＝列號＋14 欄原始值＋原因（多條以「；」串接）；檔名 `匯入問題報告_YYYYMMDD.csv`。

## 7. 實作預設（可改）

- 上限：5 MB／5,000 列。
- 預覽表格顯示全部欄位、虛擬滾動。
- 範本來源：與 `docs/資產匯入範本.csv` 相同內容。
- 匯入完成不清空現有搜尋／篩選條件。

## 8. 決策出處

- `docs/adr/0008`：格式與寫入語意（含「v4 一律保留、無 MAC 則手動」的理由）。
- `docs/adr/0006`：結構錯誤阻擋、語意警示不擋。
- `docs/adr/0007`：位址已被指派時的提示資訊樣式。
- `CONTEXT.md`：匯入／範本／問題列報告詞彙。
