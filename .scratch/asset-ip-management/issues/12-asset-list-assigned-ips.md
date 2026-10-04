# 12: 資產清單「已指派 IP」欄與 IP／MAC 搜尋

**What to build:** 資產管理清單顯示每台資產的全部已指派位址（v4 先、v6 後，同列並排），並讓關鍵字搜尋能以 MAC 或已指派 IP 找到資產——補齊 spec §2.1 已定案、程式尚未實作的部分。

**Blocked by:** None (can start immediately)

**Status:** done

- [x] 資產清單新增「已指派 IP」欄，位置在「位置」欄之後；列出該資產所有已指派位址（跨介面、跨網段、v4／v6 並存）
- [x] 每筆位址一格 chip、同列並排、過多自動換行；v4 在前、v6 在後，同地址族依位址數值排序；未指派顯示「—」
- [x] 欄位為純顯示：不可點擊、不列入排序白名單、不顯示用途／hostname
- [x] 關鍵字 `q` 以子字串、大小寫無關比對 MAC 與已指派位址（v4／v6）；與既有篩選（位置／廠牌／標籤）可組合
- [x] 後端整合測試涵蓋：無指派、單筆、雙棧、多介面多筆、q 搜尋 MAC／IP／無結果；前端 typecheck／lint 全綠

## Comments

實作完成（commit `705b629`）。

- 後端：
  - `src/assignments.rs` 新增 `list_for_assets`：以單一 `IN (...)` 查詢取當頁資產（經由介面）的全部指派位址，回傳 `HashMap<i64, Vec<String>>`；每組位址依顯示序排序（v4 先、v6 後，同族依位址數值，以 `address_sort_key` 解析 `IpAddr`），並補 1 個單元測試。
  - `src/api/assets.rs`：`GET /assets` 清單列改用 `AssetListRow` 包裝（`#[serde(flatten)]` 維持資產欄位形狀＋`assigned_ips: string[]`），先取當頁資產、再以一筆 `list_for_assets` 補齊已指派位址。POST／PATCH／詳情的 `Asset` 形狀不變。
  - `src/assets.rs` `push_filters`：`q` 於既有六個文字欄位外，補 `EXISTS` 子查詢比對 `interfaces.mac` 與 `interfaces→ip_assignments.address`（維持 LIKE 子字串、`escape_like` 轉義、大小寫無關；EXISTS 避免 JOIN 造成列重複）。
- 前端：
  - `src/api/assets.ts` 新增 `AssetListRow extends Asset`（`assigned_ips: string[]`），`AssetPage.items` 改用該型別；`Asset` 本身維持原狀，既有 POST／PATCH 回應與指派對話框搜尋用途不受影響。
  - `src/pages/AssetsPage.vue`：於「位置」欄後新增「已指派 IP」欄（`sortable: false`、純顯示）；每筆位址一 chip、同列自動換行、未指派顯示「—」；搜尋框 placeholder 補「MAC／已指派 IP」。
- 測試：`backend/tests/assets.rs` 新增 2 個整合測試（另補介面／網段／指派 helpers）：
  - `list_rows_include_all_assigned_ips`：無指派（空陣列）、單筆、多介面多網段雙棧（`["10.0.0.2", "10.9.0.2", "fd00::a", "fd00:1::5"]`，指派順序刻意與顯示序不同）、清單保留原資產欄位、POST／PATCH 回應不含 `assigned_ips`。
  - `q_matches_mac_and_assigned_ips`：MAC 精確／片段、大小寫無關；已指派 IPv4／IPv6 子字串（含大寫 v6）；與位置篩選組合（AND）；無結果。
- 驗收：`pnpm test`（60 單元＋所有整合全綠，`assets.rs` 15 個）、`cargo fmt --check`、`pnpm --filter frontend typecheck`、`pnpm lint:check` 全綠；另以暫存 SQLite 檔啟動後端 smoke（建立資產＋介面＋v4/v6 指派，確認清單 `assigned_ips` 排序、q=MAC／q=IPv6 命中、`%` 仍為字面），結束後已停止並刪除暫存檔。
- 取捨：
  - 清單僅回傳位址字串陣列（不含用途／hostname），資產詳情仍為完整 `assignments` 物件。
  - 位址排序在 Rust 端解析 `IpAddr` 以數值排序（SQLite 無 inet 型別）；無法解析的異常值排最後。
- 實作 commit：`705b629`（`12 資產清單已指派 IP 欄與搜尋：清單聚合指派位址、q 補 MAC／IP 比對（後端＋前端）`）
