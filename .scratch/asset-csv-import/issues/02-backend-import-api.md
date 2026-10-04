# 02: 後端匯入——CSV 解析、逐列驗證、dry_run 預覽與單一交易寫入

**What to build:** 新增 `POST /api/v1/assets/import?dry_run=true|false`（multipart、檔案欄位 `file`）。上傳 CSV 回傳逐列報告：偵測編碼（UTF-8／BOM／Big5）、忽略的未知欄位、summary（total／ok／warnings／errors）、每列 14 欄解析值、狀態與問題（severity／code／field／message）。`dry_run=false` 且無結構錯誤時，在單一交易建立資產＋`eth0` 介面＋IPv4／IPv6 指派，回傳 `committed` 與 `created` 統計；有任何結構錯誤則整批不寫入、回報錯誤列。欄位格式與寫入語意依 `docs/adr/0008`；API 形狀依 `.scratch/asset-csv-import/spec.md` §5。

**Blocked by:** 01

**Status:** ready-for-agent

- [ ] 解析：第一列標題、trim 後比對且英文不分大小寫、順序不拘；缺「描述」或「位置」標題、重複標題＝400；未知標題忽略並列入 `ignored_headers`；列號＝資料記錄序號＋1；空行忽略；半形逗號、RFC 4180 引號、LF／CRLF；上限 5 MB／5,000 列（超過 400）
- [ ] 編碼：UTF-8（含 BOM）優先，失敗改用 Big5，回應附 `encoding`；UTF-16 或無法解讀回 400 並提示另存 UTF-8／Big5
- [ ] 列級結構錯誤（任一即整批不匯入）：描述／位置必填；購置日期三種格式正規化；年限非負整數；MAC 正規化與格式；IPv4 須為 host、屬既有 v4 網段、非 pool、未被指派、檔內不重複；IPv6 須屬既有 v6 網段、未被登錄、檔內不重複；hostname 僅「MAC＋IPv4」列可填
- [ ] 語意警示（不擋）：重複財產編號、重複設備序號、重複 MAC（檔內或既有）
- [ ] 「位址已被指派」訊息附目前指派對象（資產描述／位置、介面名稱／MAC），比照 ADR-0007 提示資訊
- [ ] 用途推導與介面規則：mac／ipv4／ipv6／hostname 任一有值即建 `eth0`（名稱固定、無備註）；全空只建資產；IPv4 有 MAC→保留、無 MAC→手動；IPv6 恆手動
- [ ] `dry_run=true` 不寫任何資料；`dry_run=false` 成功回 `committed=true`＋`created{assets,interfaces,assignments}`；有錯誤回 `committed=false`、`created=null`、HTTP 200
- [ ] 整合測試：編碼與標題處理、14 欄規則逐項、警示、檔內重複 IP／MAC、上限、成功匯入（資產＋介面＋指派落地，可用既有 GET 驗證）、錯誤時交易回滾無殘留、preview 後資料變動導致提交失敗
