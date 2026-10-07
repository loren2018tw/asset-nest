# ADR-0023：完整同步建立缺少的受管網段

- 狀態：已接受
- 日期：2026-10-07

## 背景

ADR-0013 將受管網段的網段層設定（位址池、gateway）納入完整同步，但明定「**網段本身不由本系統增刪**：受管網段仍須先存在於 Kea」。Kea 端缺少對應 `subnet-id` 的網段時，整段跳過並回報（ADR-0011）。實務代價：管理者必須先在 Kea 設定檔手寫 `subnet4`、記下 `id`，再回 asset-nest 逐筆填 `kea_subnet_id`——順序倒置且易錯；且 `kea_subnet_id` 不具可攜性：網段 CSV（ADR-0009）在空 Kea 上無法靠同步重建。

Kea 3.0 起 `subnet_cmds` hook 為開放原始碼（`deploy/install.sh` 已載入 `libdhcp_subnet_cmds.so`），提供 `subnet4-add`：`id` 必須明示且與所有網段唯一、前綴不得重複（違反即回錯、不新增）；可帶 `pools` 與 `option-data`，但不得帶 `reservations`（保留一律走 `reservation-*`）；未指定參數會與伺服器當前全域設定合併處理繼承（Kea 原始碼 `subnet_cmds.cc`）；只影響目標網段。

## 決策

- **完整同步建立缺少的受管網段（僅增）**：以 `kea_subnet_id` 為鍵；Kea 端（`config-get`）查無該 `id`、且無相同 CIDR（沿用 `same_network` 正規化比對）掛於其他 `id` 時，計畫列為「新增網段」，確認後以 `subnet4-add` 建立。
- **建立內容僅含本系統管理欄位**：`id`＝`kea_subnet_id`、`subnet`＝CIDR、`pools`＝位址池逐段（`start - end`）、`option-data` 的 `routers`＝gateway（未設則不帶）；其餘參數不送，依 Kea 全域繼承與預設。
- **衝突即錯誤、不自動修復**：相同 CIDR 已由其他 Kea 網段使用 → 計畫預檢錯誤（訊息指出占用者 `id`），不送 `subnet4-add`；`id` 存在但 CIDR 不符 → 維持整段錯誤；一律不修改、不刪除 Kea 既有網段。
- **僅在完整同步**：先產生唯讀計畫、確認後套用；網段建立與編輯（PATCH）皆不即時推送（同 ADR-0013 對網段層的處置）。
- **套用順序**：逐網段建立缺少者（序列）→ 保留三相位（新建成功者同回合併推保留）→ 既有網段網段層設定更新（新建者跳過）→ 有任何成功變更才 `config-write` 一次。
- **失敗策略**：建立失敗記該網段錯誤、跳過其保留與設定、不阻擋其他網段；修復靠重跑完整同步（同 ADR-0011）。
- **不刪除**：Kea 端多出的網段、asset-nest 刪除或改號後遺留的網段，一律不自動刪除；孤兒現象於文件與 UI 說明。
- **`kea_subnet_id` 範圍驗證**：新增 `0 < id < 4294967295`（Kea 限制）驗證；表單與 CSV 匯入提示補「Kea 尚無此 subnet-id 時，完整同步會建立」。

## 理由

- `kea_subnet_id` 成為真正的規劃起點：在一個地方規劃完整網段（CIDR、池、gateway），一次同步落地；網段 CSV 匯出／匯入因此可作為空 Kea 的重建路徑。
- `id`／前綴唯一性由 Kea 把關；相同 CIDR 的預檢讓乾跑階段即可見衝突（通常是 `kea_subnet_id` 填錯），不冒險動 Kea 既有設定。
- 建立先於保留：保留以 `subnet-id` 為歸屬，先確保網段存在、失敗則整段跳過，避免把保留推往不存在的網段。
- 僅增不刪：`subnet4-del` 不清理該網段的租約與保留，Kea ARM 明白警告殘留衝突；刪除風險遠高於收益，維持人工處理。
- 只送管理欄位：與本系統「只管理池與 gateway」的界線一致；全域繼承由 Kea 處理，無需快照。

## 後果

- ADR-0013「網段本身不由本系統增刪」的「增」由本 ADR 取代（「刪」維持）；ADR-0012「網段仍由管理者維護」同此擴充，原文不動。
- GLOSSARY「受管網段」「完整同步」詞條隨本 ADR 更新。
- README、`deploy/install.sh` 安裝後提示、`.scratch/kea-subnet-sync/spec.md` 的單一流程敘述改為 Kea-first 與 asset-nest-first 並存（實作範圍見 spec）。
- 同步計畫／結果與 Kea 同步對話框新增「新增網段」呈現（欄位契約見 spec）；套用按鈕條件納入該差異；`KeaStatusPage` 的網段數不一致提示鬆綁（可能只是尚未同步）。
- `subnet4-add` 依賴 `subnet_cmds` hook（deploy 已載入）；未載入時建立失敗記入報告（同 `subnet4-update`）。
- 若維運另有工具直接管理 Kea 網段，缺少的受管網段會被完整同步補建；請先知會（單一寫入者精神，同 ADR-0010／0011）。
- 未受管網段與 IPv6 完全不變。
