# 03 — 整合驗證（真實清冊與檢核）

Status: done
Blocked by: 01, 02

## 目標

以真實財管清冊做端到端驗證，確認規格落地、唯讀、結果可核對。

## 步驟

1. 讀 `.scratch/inventory-audit/spec.md`、`docs/adr/0024`、`GLOSSARY.md`「清冊稽核詞彙」逐條核對。
2. 執行：
   - `cargo fmt --manifest-path backend/Cargo.toml -- --check`
   - `pnpm test`
   - `pnpm --filter frontend typecheck`
   - `pnpm lint:check`
   - `pnpm build:frontend`
3. 端到端（`pnpm dev`＋開發庫）：上傳 `~/Downloads/asset-data/資訊組財產清冊.xls` 與 `資訊組物品清冊.xls`：
   - 期望（2026-10-09 開發庫基準）：逐檔 597／172 列、略過 0；summary＝list 769（財產 597＋物品 172）、system 874、matched 764、missing_in_system 5、missing_in_list 110。
   - missing_in_system 五筆：3140101-03 3001388、60102 6005129、60102 6005255、60106 6000151、60114 6005242。
   - missing_in_list 110 筆中：100 筆含「(併主機)」、3 筆 x 前綴、1 筆「無帳」、6 筆純編號（3000874、3001678、6001551、6005850、6005851、6005852）；抽查原文與欄位正確。若開發庫已變動，以結構正確為準並回報差異。
   - 錯誤案例：非 Excel（如 CSV）、無「財產編號」表頭、0 列、11 檔、>5 MB → 整批拒絕、`details.files` 逐檔訊息正確。
   - 唯讀：稽核前後 `assets` 筆數／內容不變。
4. 交叉檢查：CSV 匯出以 Excel 可開（BOM）、筆數與畫面一致；`/assets?q=` 連結命中該筆；側欄 active 正確。
5. `git status --short`／`git diff --stat` 檢視變更範圍（不得動 `agent/`、`deploy/`；應含 spec、`docs/adr/0024`、`GLOSSARY.md`、tickets；README「Tech Stack」功能清單補「清冊稽核」）。

## 產出

- 驗證報告（回覆）：每項結果、發現的問題與修正（票 01–02 範圍內可直接修，否則回報）。
- 若有無法通過項目：列出檔案、行號、失敗輸出與建議，不要 `git commit`。

## 注意

- 不要 `git commit`；不要主動改規格或 GLOSSARY（除錯字）。

## Comments

### 2026-10-09（票 03 整合驗證）

**結論：全部通過（含真實兩檔數字逐項命中基準）；無程式修正，僅補 README「Tech Stack」條目。** 原始紀錄與輸出：`/tmp/opencode/ia-verify/`（`*.log`、`e2e-*.json`、`err*.json`、驗證腳本）。

**A. 指令組（全數 exit 0）**

| 指令 | 結果 |
| --- | --- |
| `cargo fmt --manifest-path backend/Cargo.toml -- --check` | exit 0（無輸出） |
| `pnpm test`（cargo test） | exit 0；**425 passed／0 failed**（29 個 test target；含票 01 新單元 11＋整合 8） |
| `pnpm --filter frontend typecheck` | exit 0（`vue-tsc --noEmit` 無輸出） |
| `pnpm lint:check` | exit 0；oxfmt「All matched files use the correct format.」、oxlint 無警告 |
| `pnpm build:frontend` | exit 0；Build succeeded；`assets/InventoryAuditPage-B3zUs_SJ.js` 12.79 KB |

**B. 端到端（未動 live DB、未用 8080）**

設定：`cp asset-nest.db /tmp/opencode/ia-verify/dev-copy.db`（複製當下無 `-wal`／`-shm`）；`DATABASE_URL=sqlite:///tmp/opencode/ia-verify/dev-copy.db`、`BIND_ADDR=127.0.0.1:18081`、`AUTH_USERNAME=admin AUTH_PASSWORD=admin`；curl 登入保存 cookie。上傳 `資訊組財產清冊.xls`＋`資訊組物品清冊.xls`（同請求、`file` 重複）。

- 逐檔：財產 597／物品 172、`skipped` 皆 0 —— **actual ＝ expected**。
- summary：`list_total 769（property 597＋item 172）、system_total 874、matched 764、missing_in_system 5、missing_in_list 110` —— **actual ＝ expected，全項一致**。
- `missing_in_system`（正規化鍵、排序升冪）恰 5 筆：`3140101-03 3001388`（name「個人電腦」、location「電腦教室」非空、note null）、`60102 6005129`、`60102 6005255`、`60106 6000151`、`60114 6005242`；物品列保留 `601xx` 分類前綴與多行原文（JSON 內為 `60102\r\n\r\n6005129` 等）。
- `missing_in_list` 恰 110 筆：100 筆含「(併主機)」、3 筆 x 前綴（`x1140108`／`x1140109`／`x1140619`）、1 筆「6002695 無帳」、6 筆純編號（3000874／3001678／6001551／6005850／6005851／6005852）；抽查 `3140101-03  3001030(併主機)` 保留雙空格原文；鍵升冪、無空編號；與 spec §5.2 範例第一列（asset_id 877、3000874、電腦機房(機櫃)、note 166823）完全一致。
- 單檔 sanity（僅財產清冊）：list 597、matched 596、missing_in_system 1、missing_in_list 278。
- 唯讀：上傳前後 copy DB `sha256＝057c24a5…`、`assets COUNT(*)=1741`、非空編號 874、前 5 列內容不變 —— 位元級不變。正本 `asset-nest.db` sha256／mtime（16:47:57）亦不變。
  - 註：以 Python `mode=ro` 開啟正本量測時，SQLite 產生暫存 `asset-nest.db-wal`（0 B）／`-shm`（32 KB）；量測後已刪除，正本主檔未受影響。

**C. 檔級錯誤（全數 400、整批拒絕）**

回應皆為 `{error:"validation_error", message:"…", details:{field:"file", files:[{filename,message}]}}`：

1. `bad.csv` →「僅接受 .xls／.xlsx 格式的檔案」。
2. 無表頭 xlsx →「找不到「財產編號」表頭」。
3. 僅表頭 0 列 xlsx →「沒有資料列」。
4. 11 檔（同一有效檔重複）→ message「一次最多上傳 10 個檔案，整批不稽核」＋ `files` 11 筆逐檔「超過單次 10 個檔案上限」。
5. 有效 `.xlsx` 以未使用 ZIP 條目撐至 5,283,832 B（>5 MB）→「檔案超過 5 MB 上限」。

加驗：混合好壞檔（valid＋bad.csv）→ 400 且僅列壞檔；兩個壞檔 message 以「；」串接；未登入請求 → 401。

**D. 交叉檢查**

- **CSV 匯出（真 util）**：以 `frontend` 內既有 Vite 8 的 SSR module runner 直接載入 `frontend/src/utils/inventoryAudit.ts`（temp 腳本於 `/tmp`）——欄序（系統缺少 10 欄／清冊缺少 5 欄，無「操作」）、僅 CRLF、BOM `EF BB BF`、null→空、同鍵重複「是」／空、逗號／引號／換行 RFC 4180 轉義、檔名 `清冊稽核_系統缺少_20261009.csv`／`清冊稽核_清冊缺少_20261009.csv` —— 全數 PASS。
- **`/assets?q=`**：`InventoryAuditPage` 以 router `query` 傳值（vue-router 序列化即等同 encodeURIComponent：CRLF→`%0D%0A`、`&`→`%26`，round-trip 原文一致；顯式再編碼會雙重編碼）；`AssetsPage` `onMounted` 讀 `route.query.q` 初始化搜尋。瀏覽器點擊列為手動項。
- **文件一致性**：ADR-0024 實測數字（769、874、(併主機) 100／x 3／無帳 1、96 組成對）與 DB 實測一致；GLOSSARY「財產編號／清冊／清冊稽核」在位且語意相符；spec §2–§6 逐條與實作相符（差異僅下方觀察項）；「保管人員」依 ADR 不入詞彙。
- **README**：`# Tech Stack` 資產 bullet 補「清冊稽核（財管盤點清冊 `.xls／.xlsx` 直讀比對；唯讀）」（本票要求）。
- **`git status --short`／`git diff --stat`**：僅預期檔案——backend（模組、測試、fixtures、Cargo）、frontend（頁面、API、util、client、router、sidebar、AssetsPage）、`GLOSSARY.md`、`README.md`、ADR-0024、`.scratch/inventory-audit/`；`agent/`、`deploy/` 無任何變更。

**E. 修正與觀察**

- 修正：無程式修正（票 01–02 範圍內無發現缺陷）；僅 README 功能清單補條目。
- 觀察（皆不阻擋，供驗收判斷）：
  1. 側欄無「資產管理」群組標題，實作為「資產管理／資產借還／清冊稽核」平行項目（同票 02 假設，active＝`/inventory-audit` 前綴）。
  2. CSV 匯出範圍＝目前篩選後列（「所見即所下載」）；spec §6 未明定，待驗收確認。
  3. 多行編號原文為 CRLF（BIFF 原始形式），UI `pre-wrap`、CSV 以引號包住；符合「原文保留、不清洗」。
  4. GLOSSARY「借用人」`_Avoid_: …保管人`，惟稽核表沿用清冊原欄名「保管人員」；依 spec §2／ADR-0024 屬清冊原始欄位，未改詞彙。

**待使用者手動驗證（瀏覽器 UI）**

1. 側欄「清冊稽核」（`fact_check`）出現、`/inventory-audit` 前綴高亮、未登入導向登入頁。
2. 上傳互動：多選／拖放、非 `.xls/.xlsx` 拒收警告、佇列檔名＋大小、可移除、無檔停用、請求中 loading。
3. 成功結果 UI：摘要 chips、逐檔摘要、兩表欄位／`同鍵重複`／篩選筆數／虛擬滾動／空狀態。
4. 錯誤 UI：頁面錯誤區塊（逐檔）＋ negative `$q.notify`，不產生結果。
5. CSV 實際下載（瀏覽器）：BOM／CRLF／中文檔名，並確認「篩選後下載」範圍是否符合驗收期待。
6. 「在資產清單檢視」點擊 → `/assets?q=<編號原文>`，搜尋框帶入且清單依該字串查詢。
7. 「重新上傳」清空佇列回到上傳態；失敗後修正檔案可重送成功。
