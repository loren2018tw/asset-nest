# 07: 衝突標記與網段統計

**What to build:** 語意衝突的偵測與呈現(標記不阻擋,見 ADR-0006):IpInPool、IpOutOfSubnet(含 v6)、DuplicateHwAddress;網段列表的已用/總數/衝突數。

**Blocked by:** 05 IP 指派(手動／保留)、06 v6 位址登錄制

**Status:** done

- [x] IP 列以徽章顯示命中規則;並可辨識是哪一條衝突
- [x] 建立/更新指派時若產生衝突,回應附警示訊息且不阻擋儲存
- [x] 網段列表顯示「已用」(手動＋保留)、「總數」(v4 為 host 數;v6 顯示「已登錄 N」)、衝突數
- [x] 網段編輯造成既有指派出界或落池時,改以衝突徽章反映(不阻擋)
- [x] IpInUse 由「同網段不重複指派」結構規則保證,不另做檢查
- [x] 後端整合測試涵蓋三條規則的觸發與不阻擋行為

## Comments

實作完成（commit 訊息：`07 衝突標記與網段統計：三條語意規則、徽章與已用/總數（後端＋前端）`）。

- 後端：
  - 新增 `src/conflicts.rs`：三條語意規則集中偵測（標記不阻擋，見 ADR-0006）。`IpInPool`＝指派位址落在該網段任一 pool 內（僅 v4）；`IpOutOfSubnet`＝位址不在 CIDR 內（v4／v6 皆適用）；`DuplicateHwAddress`＝同一網段同 MAC 出現多筆保留（purpose=reservation；同一 MAC 跨多介面亦計），所有同 MAC 保留列一併標記。代碼依固定順序輸出（IpOutOfSubnet、IpInPool、DuplicateHwAddress）。`detect` 回傳命中列、`by_address` 供列填入、`warnings_for` 產生儲存警示（中文訊息，沿用票 02 的 `Warning` 型別）。偵測即時計算、無快取：網段編輯與取消指派後，下一次讀取即反映最新衝突。
  - `GET /subnets/{id}/ips`：每列 `conflicts` 填入命中代碼（v4 枚舉列與 v6 登錄列皆適用）。v4 清單改為聯集「host 範圍列＋所有指派列」：因網段縮小而出界的指派仍會出現（票 05 備註的暫時缺漏已補），出界列以數值順序合併、搜尋／狀態篩選／分頁與總數一致涵蓋；無出界列時保留票 04 的算術位移快速路徑。`list_v4` 抽出 `V4Scanner` 統一掃描邏輯。
  - `PUT`／`POST /subnets/{id}/ips...`：回應改為指派欄位攤平＋`warnings`（沿用票 02 `InterfaceResponse` 模式）；命中衝突時警示、不阻擋（HTTP 200／201，資料已儲存）。
  - 更新既有指派（同介面同位址）不再重驗出界／落池：`validate_address` 新增 `existing` 參數，僅新指派驗證 host 範圍與 pool；地址族與 v6 用途限制仍為結構規則。否則「pool 擴大／CIDR 縮小後改用途」會被結構驗證擋下，與本票「不阻擋」矛盾。
  - `DELETE` 維持 204；衝突於下一次讀取重算（DuplicateHwAddress 因刪除自然消失）。
  - `GET /subnets` 的 `SubnetSummary` 補 `used`／`total`／`conflicts`：`used`＝static＋reservation 指派數；`total`＝v4 host 數（扣 network/broadcast；`/31`、`/32` 全列）、v6 已登錄數（= `used`）；`conflicts`＝命中至少一條規則的指派「筆數」（同一筆命中多條只計 1）。統計於列表時逐網段即時計算。
- 前端：
  - `IpListPage.vue`：新增「衝突」欄，以 warning 徽章顯示可辨識的規則名稱（池內／出界／MAC 重複）並附中文 tooltip 說明；v4／v6 皆適用。
  - `SubnetsPage.vue`：新增「已用／總數」欄（v4 顯示 `N / M`；v6 顯示「已登錄 N」）與「衝突數」欄（>0 時 warning 徽章）。
  - `AssignmentDialog.vue`：指派／登錄成功後逐條以 `$q.notify`（warning、6 秒）顯示 `warnings`，不阻擋；`api/ips.ts` 新增 `AssignmentSaved`（`Assignment`＋`warnings`），`api/subnets.ts` 補統計欄位。
- 測試：
  - 新增 `backend/tests/conflicts.rs` 6 個整合測試——pool 擴大後 IpInPool（列徽章、更新警示、統計）；v4 縮小後 IpOutOfSubnet（出界列可見、精確搜尋／狀態／分頁／總數、更新警示、統計）；v6 縮小後 IpOutOfSubnet；DuplicateHwAddress（觸發與不阻擋、同 MAC 手動設定不觸發、改用途消除、取消後重算）；衝突數為筆數非規則數；統計涵蓋 `/31`、`/32`、`/30` 與 v6。
  - `src/conflicts.rs` 6 個單元測試（三規則、多規則順序、訊息）；`src/ips.rs` 新增 3 個、調整既有測試（v4 聯集與排序、pool＋出界並存、v6 出界標記）。
- 驗收：`cargo test`（39 單元＋47 整合全綠）、`cargo fmt --check`、`pnpm typecheck`、`pnpm lint:check` 全綠；另以真實伺服器＋暫存 SQLite smoke（pool 擴大→IpInPool 徽章與統計；同 MAC 保留→DuplicateHwAddress 警示、取消後消失；清空 pool 並縮小 CIDR→出界列仍可見、更新附 IpOutOfSubnet 警示；v6 縮小→出界標記）。
- 與規格差異／取捨：
  - 衝突數採「筆數」定義（命中列數），與 IP 列徽章可見的列一一對應；同一筆同時出界且 MAC 重複仍只計 1。
  - v4 出界指派以清單聯集呈現：總數＝host 數＋出界筆數，出界列排在數值順序位置；`in_pool` 與衝突代碼可並存。
  - 更新既有指派不重驗出界／落池（新指派不變），使「先指派、後編輯網段」的流程真正不阻擋。
  - 網段編輯（PATCH）既有結構驗證不變：pool 仍須在 CIDR 內、CIDR 不得與他段重疊；僅既有指派造成的出界／落池改由標記反映。
  - IpInUse 不另實作，由 `UNIQUE (subnet_id, address)` 結構規則保證（詞彙保留供對帳階段）。
- 實作 commit：`ffc24bf`（`07 衝突標記與網段統計：三條語意規則、徽章與已用/總數（後端＋前端）`）
