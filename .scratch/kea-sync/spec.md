# Kea 保留推送與完整同步

對應票號：無（決策見 `docs/adr/0011`；控制通道見 `docs/adr/0010`、`docs/adr/0002`）。

## 目標

- 設定 `kea_subnet_id` 的 IPv4 網段視為受管；其「保留」指派單向同步到 Kea。
- 指派／改用途／改 hostname／取消指派／移轉時，即時推送單筆保留。
- 提供「Kea 同步」按鈕：先 dry-run 計畫、確認後套用完整同步（含刪除多餘）。

## 範圍

- 僅 IPv4、僅「保留（reservation）」用途；static 不經 Kea。
- 不做雙向同步、不做租約讀取（後續階段）。
- 同步狀態不落庫；失敗即時警示，修復靠完整同步。

## 後端

- `kea::http::Client`：`reservation_add`／`reservation_del`／`reservation_get_all`／
  `config_write`／`kea_subnets`（`config-get` 取 subnet-id 與 CIDR）；`result` 3 視空。
- 保留命令一律帶 `operation-target: "memory"`（操作執行中設定；host_cmds 的修改
  命令預設只寫主機資料庫，未設定時回 `Host database not available`；見 ADR-0011）。
- `kea::sync`：
  - `after_assignment_change`：單筆推送（先刪再增＋`config-write`）；觸發條件與失敗策略見 ADR-0011。
  - `plan`／`apply`：完整同步；刪除只限 hw-address＋ip-address 形式；衝突指派跳過；
    分相（刪→改→增）、併發上限 6、單項失敗續行；整批一次 `config-write`。
- API：`GET /api/v1/kea/sync/plan`、`POST /api/v1/kea/sync`；
  指派回應附 `kea_sync`（僅應同步時）；取消指派 200＋`kea_sync`。
- `AppState` 附掛 `Option<Client>`（`KEA_API_URL` 未設定時停用）。

## 前端

- 網段列表頁「Kea 同步」按鈕 → `KeaSyncDialog`：計畫計數與明細、確認套用、結果報告。
- 指派／取消成功後，`kea_sync.status === "failed"` 以 warning 提示（`utils/keaSync.ts`）。

## 測試

- 單元：`kea::sync` 差異計算（新增／更新／刪除／跳過）。
- 整合（stub Kea）：單筆推送（reservation／static／未受管）、保留→static 刪除、取消指派、
  Kea 失敗不阻擋、計畫與套用（含衝突跳過、整批單次寫檔）、未設定 Kea 回 400。
- 真機（`pnpm test:kea`）：`version-get`＋保留 roundtrip（add→get→del，不留持久變更）。

## 指令

```sh
cargo test --manifest-path backend/Cargo.toml   # 含 stub 整合測試
pnpm test:kea                                    # 真機（需 .env）
```
