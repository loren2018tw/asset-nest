# 06: v6 位址登錄制

**What to build:** IPv6 網段的 IP 管理頁:只登錄與列出已指派的位址,提供「新增位址」;用途固定手動設定,不枚舉空閒位址。

**Blocked by:** 05 IP 指派(手動／保留)

**Status:** done

- [x] v6 網段 IP 頁只列出已登錄位址,附「新增位址」入口
- [x] 新增即指派(輸入位址+選資產/介面,流程同 05);可編輯與取消指派
- [x] 用途固定為手動設定;不提供保留選項,亦無 pool 概念
- [x] 不顯示任何空閒位址
- [x] 後端整合測試涵蓋登錄、取消與 v6 結構檢查(合法 IPv6、同網段不重複)

## Comments

實作完成（commit 訊息：`06 v6 位址登錄制：登錄即指派、清單與取消（後端＋前端）`）。

- 後端：
  - `POST /subnets/{id}/ips`（新增）：v6 專用，輸入 `{address, interface_id}`；驗證位址為合法 IPv6 且在網段 CIDR 內，建立即指派（purpose 固定 static、無 hostname）。成功回 201；v4 網段回 400（field `cidr`）；重複登錄（同網段同一位址，含同一介面）回 400（field `address`＋`details.interface_id`），不採冪等更新；同介面同網段已有位址回 400（field `interface_id`＋`details.existing_address`）。
  - `GET /subnets/{id}/ips`：v6 分支由 501 改為正式實作——僅回傳已登錄（有 assignment）位址，以數值（u128）升冪排序；支援關鍵字（完整位址以解析值精確比對、可容忍不同壓縮寫法；部分字串比對位址文字與指派對象）、狀態篩選（static 全數；available／in_pool／reservation 皆空）與伺服器端分頁；`in_pool` 恆 false、狀態恆 static。無 pool、不顯示任何空閒位址。
  - `PUT`／`DELETE /subnets/{id}/ips/{address}/assignment`：改為 v4／v6 皆可（路徑位址解析為 `IpAddr`）；v6 時 PUT 僅允許 purpose static（reservation 回 400 field `purpose`）、位址須落在 CIDR 內、與網段地址族不符回 400（field `address`）；DELETE 即刪除登錄（位址文字正規化後刪除，展開寫法亦可）。`ips::list` 重構為 v4 枚舉／v6 登錄兩分支，`IpEntry.address` 型別改為 `IpAddr`。
- 前端：
  - `IpListPage.vue`：移除 v6「尚未支援」banner；v6 顯示已登錄清單＋「新增位址」按鈕（v6 篩選僅提供「手動設定」）；空清單提示引導新增；取消確認訊息改為「自登錄清單移除」；不再跳過 v6 的 API 呼叫。
  - `AssignmentDialog.vue`：新增 `family` prop；v6 新增模式提供可編輯的 IPv6 位址輸入（即時基本格式驗證，以 `new URL` 解析、支援 :: 壓縮與 IPv4-mapped）＋沿用票 05 的資產／介面選擇（含當場新增介面）；v6 隱藏用途選項與 hostname，改以唯讀欄位顯示「手動設定（static）」；v6 編輯模式（既有登錄列）僅供檢視＋取消指派（換介面＝取消＋重新指派，依 ADR-0005），不顯示儲存。`api/ips.ts` 新增 `registerIp` 與 `RegistryInput`。
- 測試：新增 `backend/tests/ip_registry_v6.rs` 5 個整合測試——登錄即指派（201／清單僅登錄位址／gateway 標記／資產詳情）、輸入驗證（v4 網段 400、非法位址、出界、network 位址可登錄、缺欄位、未知介面／網段）、唯一性與跨網段（同址不同介面、同介面同網段、同介面重複登錄、跨 v6 網段可各一、v4+v6 雙棧可並存）、PUT／取消（static 冪等、reservation 400、hostname 400、換介面 400、PUT 可建立、展開寫法刪除、404）、清單搜尋／狀態／分頁（數值排序、展開寫法精確比對、資產描述／介面／MAC 關鍵字）。另更新 `tests/ips.rs` v6 501 測試為空清單、`tests/assignments.rs` v6 斷言改為地址族不符；`src/ips.rs` 新增 4 個單元測試、`src/assignments.rs` 新增 1 個單元測試。
- 驗收：`cargo test`（31 單元＋41 整合全綠）、`cargo fmt --check`、`pnpm typecheck`、`pnpm lint:check` 全綠；另以真實伺服器＋暫存 SQLite smoke（登錄→清單→PUT reservation 400→percent-encoded 路徑取消→重複登錄 400→v4 網段 POST 400）。
- 與規格差異／取捨：
  - v6 network 位址（如 `fd00::`）可登錄：spec §7 明示 v6 無 host 扣除概念，只要是 CIDR 內合法位址即可；network/broadcast 扣除僅適用 v4。
  - `POST` 重複登錄一律回 400（即使同一介面）：登錄端點語意為「新增」，不沿用票 05 PUT 的冪等更新；PUT 仍保留冪等語意。
  - `PUT` 對未登錄的 v6 位址仍可建立指派（等同登錄；與 v4 PUT 一致），前端主流程一律走 POST。
  - v6 編輯模式無可變欄位（用途固定、hostname 不適用、介面不可更換），對話框僅供檢視與取消指派。
  - 因網段縮小而出界的登錄位址仍會列出（登錄制以指派為準），衝突標記由票 07 以 IpOutOfSubnet 呈現。
  - 網段列表「已登錄 N」統計屬票 07，本票未做。
- 實作 commit：（commit 後補）
