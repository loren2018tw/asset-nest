# 02: 網段匯入後端

**What to build:** `POST /subnets/import` 接受 CSV（UTF-8／Big5），逐列驗證後以單一交易新增網段；dry-run 提供預覽報告。

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

- [ ] 檔級規則沿用資產匯入：標題列處理、編碼偵測（回應 `encoding`）、大小／列數上限、傳輸層錯誤 400
- [ ] 列級結構錯誤（任一即整批不寫入）：CIDR 格式／已存在（含檔內重複）／與既有或檔內重疊（含嵌套）、gateway 不在 CIDR 內、Kea subnet-id 非正整數或重複、pool 格式／範圍／重疊錯誤、v6 帶 Kea subnet-id 或 pool
- [ ] 只新增：不更新、不刪除既有網段
- [ ] `dry_run=true` 回預覽報告（比照資產匯入：`encoding`／`ignored_headers`／`summary`／`rows`）；`dry_run=false` 成功回 `committed=true`、`created={subnets}`，於單一交易完成
- [ ] 領域寫入可於交易內重用（必要時先做最小前置重構；比照資產匯入的前置重構）
- [ ] 後端整合測試涵蓋各結構規則、檔內重複、v6 限制、dry-run 不寫入、正式匯入原子性
