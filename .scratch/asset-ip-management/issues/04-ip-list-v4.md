# 04: v4 IP 清單(唯讀)

**What to build:** 自網段列表進入該網段的 IP 管理頁;v4 位址全枚舉、狀態推導與瀏覽(尚無指派功能)。

**Blocked by:** 03 網段設定

**Status:** done

- [x] v4 網段列出全部 host 位址:扣除 network/broadcast,`/31`、`/32` 全數列出;可為任意前綴
- [x] IP 以數值排序;伺服器端分頁(預設 50 筆/頁)與 IP 關鍵字搜尋
- [x] 落在 pool 範圍的列標示「池內」,且無編輯入口;gateway 位址顯示標記
- [x] 未被指派且不在 pool 內的列顯示「可用」
- [x] 後端整合測試涵蓋枚舉邊界(/31、/32、一般前綴)與 pool/gateway 標示

## Comments

實作完成（commit 訊息：`04 v4 IP 清單：位址枚舉、pool/gateway 標示與分頁瀏覽（後端＋前端）`）。

- 後端：新增 `src/ips.rs` 領域模組（不建表，由 Subnet 範圍伺服器端推導）：
  - `HostRange`：v4 host 範圍，扣除 network/broadcast；`/31`、`/32` 全數列出；任意前綴（含 `/0`）以 u64 位移計算總數與第 n 個位址。
  - `GET /subnets/{id}/ips`（`src/api/ips.rs`）：`q` 關鍵字（可解析為完整位址時精確比對，否則對位址文字子字串比對）、`page`／`per_page`（預設 50、上限 200、下限 1，比照 `/assets`）；列以數值升冪（枚舉順序即排序）；回應 `{items, total, page, per_page}`，列含 `address`、`in_pool`、`is_gateway`、`status`（本票為 `available`／`in_pool`）、`purpose`（預留票 05 指派用途）、`conflicts`（預留票 07 衝突標記）。
  - 無關鍵字時不掃描全部位址：總數以算術計算、當頁以位移取得；完整位址搜尋亦為常數時間。
- v6：本票不處理，`GET /subnets/{id}/ips` 回 501 `{error:"not_implemented", message:"IPv6 網段的 IP 清單尚未支援（見票 06）"}`（`ApiError` 新增 `not_implemented`）；前端遇 v6 網段顯示說明 banner、不呼叫 API。
- 前端：
  - 網段列表（`SubnetsPage.vue`）CIDR 改為連結、列操作新增「IP 清單」按鈕，進入 `/subnets/:id/ips`。
  - 新增 `IpListPage.vue`：顯示 IP、Gateway 徽章、狀態／用途徽章（池內／可用）、搜尋（debounce 300）與伺服器端分頁；「指派對象」與「操作」欄位為票 05 預留（指派按鈕停用；池內列依 spec §7 無編輯入口）；側欄「IP 管理」在網段 IP 頁維持高亮。
  - `api/ips.ts`：型別與查詢封裝。
- 測試：`backend/tests/ips.rs` 6 個整合測試——一般前綴（/29）枚舉與分頁（預設／上限 200／下限 1／超出頁）、`/31`、`/32`、pool/gateway 標示（含 gateway 落 pool 時兩標記並存）、搜尋（精確／子字串／子字串分頁／無結果）、v6 501、404／400；`src/ips.rs` 6 個單元測試——範圍扣除 network/broadcast、`/31`／`/32`、任意前綴（`/30`、`/0` 總數與端點）、數值排序分頁、pool/gateway 標示、搜尋精確與子字串。
- 驗收：`cargo test`（20 單元＋28 整合全綠）、`cargo fmt --check`、`pnpm typecheck`、`pnpm lint:check` 全綠；另 `pnpm build` 成功。
- 與規格差異／取捨：
  - v6 以 501 結構化錯誤擋下（規格未定；票 06 實作登錄制時替換）。
  - 「可用」目前＝不在 pool 且未被指派；票 05 指派後改由指派資料推導 `static`／`reservation`，`purpose` 與衝突欄位已預留。
  - 搜尋語意：完整位址精確、其餘子字串（規格 §5 僅寫「IP 子字串或精確」）；子字串搜尋為線性掃描，極大前綴（如 /8 以下）成本較高，實務網段規模（/16–/32）可忽略。
- 實作 commit：`d63c39a`（`04 v4 IP 清單：位址枚舉、pool/gateway 標示與分頁瀏覽（後端＋前端）`）
