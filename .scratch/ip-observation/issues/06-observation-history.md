# 06: 觀測歷史對話框與匯出

**What to build:** 從 IP 清單開啟任一 IP 的「觀測歷史」：事件時間軸（首見、MAC 變更）與用過的 MAC 清單（各自首見／最後可見／來源）；可切到 MAC 視角查「用過哪些位址」；未知 MAC 標示「未登錄」、已知 MAC 可連到對應資產；可匯出單一 IP 的歷史 CSV。

**Blocked by:** 02 快速掃描與「最後可見」欄（核心）

**Status:** done

- [x] IP 歷史端點：現況＋事件清單（時間新到舊）
- [x] MAC 歷史端點：用過的位址、每筆首見／最後可見，標示已知／未知與資產連結
- [x] CSV 匯出（`address, mac, kind, source, observed_at`；檔名沿用既有下載慣例）
- [x] 前端歷史對話框支援「以 IP 進入」與「以 MAC 進入」；IP 列新增「觀測」操作入口
- [x] 測試（端點與匯出格式）與 `pnpm lint:check`／`pnpm typecheck` 全綠

## Comments

實作摘要（commit `e8d595b`）：

- 後端：`observation.rs` 新增歷史彙總邏輯與型別（`IpHistory`／`MacHistory`／`UsedMac`／`MacSighting`／`history_export_csv`）；新路由模組 `api/observations.rs`：
  - `GET /api/v1/subnets/{id}/ips/{address}/observations`：`observed`＋`presence`（nullable）＋`events`（新到舊）＋`macs`（用過的 MAC）。
  - `GET /api/v1/subnets/{id}/ips/{address}/observations/export`：CSV；欄位 `address, mac, kind, source, observed_at`、UTF-8 BOM、事件新到舊、`filename*=UTF-8''觀測歷史_<address>_YYYYMMDD.csv`（RFC 5987）。
  - `GET /api/v1/observations/mac/{mac}`：`probe::normalize_mac` 正規化（無效回 400）＋`{mac, known, asset?, sightings}`；Interface MAC 不分大小寫比對、連結資產取第一筆。
- 彙總規則（詳見程式註解）：相異 MAC／位址＝事件 ∪ 現況 `last_seen_mac`；首見＝最早事件（無事件時取現況）；最後可見＝最晚事件，現況 MAC 相同且不舊於事件時取現況（平手現況勝）；來源取最後可見訊號。
- 前端：`api/observations.ts`（型別＋三個 helper）、`ObservationHistoryDialog.vue`（IP／MAC 雙模式、現況摘要與「未觀測／從未上線」區分、事件時間軸、用過 MAC 清單、未知標「未登錄」、IP 模式 CSV 匯出）、`IpListPage.vue` 新增「觀測」列操作（history 圖示）。
- 測試：`backend/tests/observation_history.rs` 4 個整合測試（IP 彙總與空歷史、MAC 連結與大小寫、sightings 聚合、匯出格式）。`cargo test` 285 passed／0 failed／6 ignored；`cargo fmt --check`、`pnpm lint:check`、`pnpm --filter frontend typecheck` 全綠。

後續票 08 備註：

- `ObservationHistoryDialog` props：`modelValue`、`subnetId?`、`address?`、`mac?`；emits 僅 `update:modelValue`。`mac` 有值即以 MAC 模式開啟（優先於 IP 模式）。
- 資產介面以 MAC 開啟：`<ObservationHistoryDialog v-model="open" :mac="interfaceMac" />`（無需 `subnetId`，CSV 按鈕僅 IP 模式顯示）。
- 目前沒有公開的「MAC → 資產」共用 helper：`assets_for_macs` 為 `observation.rs` 私有；MAC 端點已回 `known`／`asset`，資產端若要反向連結可直接重用該端點回應。
