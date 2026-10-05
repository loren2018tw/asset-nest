# ADR-0013：Kea 網段層設定同步（pool 與 gateway）

- 狀態：已接受
- 日期：2026-10-05

## 背景

ADR-0002 定調「Reservation 以 asset-nest 為真實來源」，ADR-0011 的完整同步只對齊保留；ADR-0012 明言「網段仍由管理者維護，asset-nest 只同步保留」。但領域上 **DHCP 位址池（pool）就是「Subnet 內保留給 Kea 動態配發的位址範圍」**（見 GLOSSARY），asset-nest 的網段設定早已是管理者維護 pool 的地方；真機實測即出現 asset-nest 有 pool、Kea `subnet4[].pools` 為空的情形。手動兩邊維護必然漂移。

Kea 3.0 起 `subnet_cmds` hook 為開源（隨 `isc-kea-hooks` 安裝），提供 `subnet4-get`／`subnet4-update`：可只取代單一網段、不影響其他網段與其他設定（見 Kea 3.2 ARM 16.27），比整份 `config-set` 安全。

## 決策

- **受管網段的網段層設定納入完整同步**：`subnet_pools` 對齊 `subnet4[].pools`；`subnet.gateway` 對齊 `subnet4[].option-data` 的 `routers` 選項。
- **網段本身不由本系統增刪**：受管網段仍須先存在於 Kea（`kea_subnet_id`＋CIDR 相符），本系統只更新其 pool 與 gateway。
- **嚴格對齊（以 asset-nest 為準）**：Kea 端多出的 pool 刪除；asset-nest 未設 gateway 時移除 Kea 的 `routers`。高風險動作一律先出現在 dry-run 計畫，經確認才套用。
- **只在完整同步推送**：網段編輯（PATCH）不即時推送；即時推送維持只針對保留（ADR-0011）。
- **通道**：`subnet_cmds` hook 的 `subnet4-update`（自 `config-get` 取得的原始網段物件整段回寫，只改 `pools` 與 `routers` 條目），全部成功後與保留變更合併一次 `config-write`；`deploy/install.sh` 載入 `libdhcp_subnet_cmds.so`。
- **pool 比對以範圍正規化**：Kea 的 CIDR 形式 pool 以整段位址展開比較（v4 prefix pool 可配發 network／broadcast，依 Kea ARM）；同範圍且 Kea 端帶額外屬性的條目原樣保留、不重建。
- 失敗策略不變（ADR-0011）：單一網段更新失敗記入報告、不阻擋其他網段與本地資料；修復靠重跑完整同步。

## 理由

- pool 是 DHCP 服務的實際配發範圍，讓 asset-nest 同時是保留與 pool 的真實來源，管理者只需在一個地方維護；gateway 與 pool 同屬網段層、一併對齊可避免只做一半。
- `subnet4-update` 以現有設定物件回寫，只動目標欄位語意、影響面小於整份 `config-set`，也不需要付費 hook。
- 嚴格對齊與保留同步一致，漂移有明確的收斂方向；dry-run＋確認讓刪除可被檢視。
- 不即時推送網段層，避免編輯到一半的 pool 立即影響執行中的 DHCP。

## 後果

- Kea 需載入 `subnet_cmds` hook（`isc-kea-hooks` 已含；既有安裝需手動加入 `hooks-libraries` 或重跑安裝腳本）。
- 若維運流程另有工具直接改 Kea 的 pool／routers，該變更會被下一次完整同步覆蓋；請先知會（同 ADR-0010／0011 的單一寫入者精神）。
- pool 條目帶 `client-classes` 且範圍需變更時，重建會遺失該屬性（同範圍不重建）；見 spec 限制。
- 網段層差異與保留共用同一個同步計畫／按鈕，前端對話框需呈現兩類計數與明細。
- ADR-0011 的「完整同步只對齊保留」與 ADR-0012 的「網段仍由管理者維護，asset-nest 只同步保留」由此擴充（受管網段的網段層設定亦納入同步），其餘內容不變。
