# 05: IP 指派(手動／保留)

**What to build:** IP 管理頁的指派流程:把一個可用 IP 指派給資產的介面,用途為手動設定或 DHCPv4 保留;可取消指派。指派對象是 Interface(ADR-0005)。

**Blocked by:** 02 資產介面管理、04 v4 IP 清單(唯讀)

**Status:** done

- [x] 可用列有編輯入口;對話框流程:搜尋選資產 → 選介面(或當場新增介面:名稱/MAC) → 選用途(手動/保留;無 MAC 時保留停用;保留可填 hostname)
- [x] 可取消指派;取消後該位址回到「可用」
- [x] 結構規則阻擋:保留需介面有 MAC;同一 Subnet 同一位址不得重複指派;同一介面在同一 Subnet 至多一個位址(跨 Subnet 含 v4+v6 可各有)
- [x] IP 列顯示指派對象:資產描述、位置、介面名稱/MAC
- [x] 資產對話框以唯讀方式顯示已指派 IP;指派一律在 IP 管理頁操作
- [x] IP 值不可修改;池內列的編輯入口維持停用
- [x] 後端整合測試涵蓋指派、取消、各結構規則

## Comments

實作完成（commit 訊息：`05 IP 指派：手動／保留指派、取消與指派對象顯示（後端＋前端）`）。

- 後端：migration `0005_ip_assignments.sql`（依 spec §2.5：`UNIQUE (subnet_id, address)`、`UNIQUE (interface_id, subnet_id)`、`purpose CHECK ('static','reservation')`、`interface_id` FK ON DELETE CASCADE、`subnet_id` FK 不連動）；新增 `src/assignments.rs` 領域模組與 API：
  - `PUT /subnets/{id}/ips/{address}/assignment`：指派／改用途（`{interface_id, purpose, hostname?}`）。結構驗證 400＋明確 `details`：保留需介面有 MAC（field `purpose`）、同一網段位址已指派給其他介面（field `address`＋`details.interface_id`）、同一介面同網段已有位址（field `interface_id`＋`details.existing_address`）、位址不在網段 host 範圍（network/broadcast 不可指派）、pool 內不可指派、v4 限定（v6 位址或 v6 網段皆 400）；同介面同位址視為改用途（冪等更新）。資料庫 UNIQUE／CHECK 為雙保險，違反時亦回 400。
  - `DELETE /subnets/{id}/ips/{address}/assignment`：取消指派回 204；不存在回 404。
  - `GET /subnets/{id}/ips` 擴充：列含 `assignment`（asset_id、asset_description、asset_location、interface_id、interface_name、mac、hostname）與 `status`／`purpose`（static／reservation；未指派且不在 pool 為 available，池內為 in_pool）；`conflicts` 維持預留（票 07）。另依 spec §4.3 補上 `status` 篩選與關鍵字比對指派對象（資產描述／介面名稱／MAC，不分大小寫）。
  - `GET /assets/{id}` 擴充：`assignments`（含 subnet_cidr／subnet_name、address、purpose、hostname、介面名稱／MAC），供唯讀顯示。
- 前端：
  - `IpListPage.vue`：可用列（非 pool、未指派）編輯入口啟用；已指派列顯示資產描述＋位置＋介面名稱／MAC（＋hostname），並提供「取消指派」（確認對話框）；池內列編輯維持停用；新增狀態／用途篩選。
  - 新增 `AssignmentDialog.vue`：搜尋選資產（`GET /assets?q=`，debounce）→ 選介面（或當場新增介面：名稱／MAC，`POST /assets/{id}/interfaces`，成功自動選取）→ 用途（無 MAC 時保留停用；保留可填 hostname）→ 指派；已指派列以編輯模式開啟，可改用途／hostname 或取消指派。IP 值唯讀不可修改。
  - `AssetFormDialog.vue`：新增「已指派 IP（唯讀）」區塊（自 `GET /assets/{id}` 載入），每列連往該網段 IP 管理頁；指派一律在 IP 管理頁操作。
  - `api/ips.ts` 新增 `assignIp`／`cancelAssignment` 與型別；`client.ts` 新增 `apiPut`；`api/assets.ts` 新增 `AssetAssignment`。
- 測試：`backend/tests/assignments.rs` 8 個整合測試——指派／改用途／冪等／取消／404、保留需 MAC、位址規則（出界、network/broadcast、pool、非法與 v6 位址、v6 網段、gateway 可指派）、重複位址與同介面同網段規則（含跨網段可各一）、兩條 UNIQUE 與 purpose CHECK 直寫驗證、刪除介面連動刪除（含刪除資產連動）、搜尋與狀態篩選、未知資源與缺漏欄位；`src/assignments.rs` 2 個單元測試（輸入驗證、hostname 僅限保留）；`src/ips.rs` 新增 4 個單元測試（指派狀態與對象、狀態篩選、關鍵字比對指派對象、篩選值解析）。
- 驗收：`cargo test`（26 單元＋36 整合全綠）、`cargo fmt --check`、`pnpm typecheck`、`pnpm lint:check` 全綠；另 `pnpm build` 成功、以真實伺服器＋暫存 SQLite 手動 smoke（指派→改用途→清單／資產詳情顯示→pool 阻擋→取消）通過。
- 與規格差異／取捨：
  - 換介面不支援直接 PUT：位址已指派給其他介面時 400 並提示先取消（依 CONTEXT.md「換介面即取消後重新指派」；`details.interface_id` 附目前指派）。
  - `hostname` 僅限保留用途；手動設定帶非空 hostname 回 400（規格未明文，屬結構層防呆；改為手動且未帶 hostname 時自動清除）。
  - v6 指派回 400（票 06 實作登錄制時替換）；衝突標記與儲存警示（含 DuplicateHwAddress）依票 07。
  - 已指派位址落在 host 範圍外（網段縮小）暫不出現在清單，由票 07 以 IpOutOfSubnet 處理。
  - 搜尋／狀態篩選為 spec §4.3 補齊（票 04 僅 IP 關鍵字）。
- 實作 commit：`45c3fef`（`05 IP 指派：手動／保留指派、取消與指派對象顯示（後端＋前端）`）
