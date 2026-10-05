# ADR-0011：保留推送與完整同步

- 狀態：已接受
- 日期：2026-10-05

## 背景

ADR-0002 定調「Reservation 以 asset-nest 為真實來源、單向推送 Kea、需冪等可重試」，但未定推送的觸發點、失敗策略與修復機制。ADR-0010 定了控制通道（Kea 內建 HTTP、Basic 認證）；`host_cmds` hook（`reservation-add/del/get-all`）已於伺服器載入，但實測其修改命令預設只寫「主機資料庫」——本伺服器未設定主機資料庫，會回 `Host database not available`；以 `operation-target: "memory"` 可改為操作執行中設定。網段設定以 `kea_subnet_id` 表示受管與否（見 GLOSSARY）。

## 決策

- **受管定義**：只有設定 `kea_subnet_id` 的 IPv4 網段參與同步；未受管網段完全不與 Kea 互動。
- **單筆推送**：指派／改用途／改 hostname（PUT）、取消指派（DELETE）、資產端移轉（PUT）時，即時推送該筆保留；僅「保留（reservation）」用途會產生 Kea 保留，「保留→static」與取消指派會刪除 Kea 保留。介面 MAC 編輯與資產／介面刪除不即時同步，交由完整同步收拾。
- **upsert 語意**：先 `reservation-del`（不存在視同已刪）再 `reservation-add`；變更期間僅毫秒級空窗。所有命令顯式帶 `operation-target: "memory"`，操作 Kea 執行中設定（host_cmds 的修改命令預設只寫主機資料庫，本系統未使用）；每次成功變更後呼叫 `config-write` 持久化到 Kea 設定檔。
- **失敗策略**：Kea 失敗不阻擋本地儲存（asset-nest 是真實來源）；指派回應附 `kea_sync: {status, message}`（僅在應同步時出現），前端以 warning 提示；由完整同步修復。取消指派回應由 204 改為 200＋同欄位。
- **完整同步**：`GET /kea/sync/plan`（唯讀計畫）＋`POST /kea/sync`（套用）。以 asset-nest 為準對齊受管網段的 Kea 保留：新增缺少、更新不同（hostname 或 MAC）、刪除多餘。刪除只限「hw-address＋ip-address」形式且不屬於任何期望保留者；非此形式的 Kea 保留、以及有衝突標記（IpInPool／IpOutOfSubnet／DuplicateHwAddress）的指派，一律跳過並列入報告。Kea 端沒有對應 subnet-id 或 CIDR 不符的受管網段，整段跳過並回報。套用分相執行（刪除→更新→新增）、併發上限 6、單項失敗續行；整批有成功變更才 `config-write` 一次。
- **同步狀態不落庫**：不新增資料表或欄位；失敗僅即時提示，漂移由完整同步計畫重新計算。

## 理由

- 即時推送讓 Kea 與指派操作同步，但不讓 Kea 的可用性成為資產管理的瓶頸（ADR-0002 的冪等與可重試）。
- 完整同步先計畫再套用，讓「刪除多餘」這種高風險動作可見後才發生；刪除範圍只限本系統能表達的形狀，避免誤刪他人維護的保留。
- 無持久同步狀態最單純；真正的修復來源是完整期望狀態（DB）與 Kea 現況的比對，而非事件歷程。

## 後果

- `.env` 需設 `KEA_API_URL`（未設定時同步停用：端點回 400、操作不帶 `kea_sync`）。
- 取消指派回應改為 200（原 204）；前端與測試已相應更新。
- Kea 設定檔由 `config-write` 週期性改寫；若維運流程另有工具編輯同一檔案或呼叫 `config-reload`，需先知會以免互相覆蓋（見 ADR-0010 後果）。
- 完整同步為同步 HTTP 請求（數秒規模）；未來保留數大幅增加時再考慮背景工作。
