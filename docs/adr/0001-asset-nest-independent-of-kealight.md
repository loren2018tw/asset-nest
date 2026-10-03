# ADR-0001：asset-nest 獨立於 Kealight

- 狀態：已接受
- 日期：2026-10-04

## 背景

Kealight 是既有的獨立工具，已能管理 Kea DHCPv4 主機保留（直接編修設定檔、control-socket 重載、租約唯讀檢視、衝突防護、備份、簡易密碼）。asset-nest（IT 資產整合管理系統）要整合資產、IP 位址與 Kea 保留同步，功能與 Kealight 部分重疊。

## 決策

asset-nest 作為全新獨立系統發展：不依賴 Kealight、不共用程式碼，Kealight 維持現狀。但領域詞彙沿用 Kealight 已定案者（Reservation／Subnet／Lease／Conflict 規則）。

Kea 整合機制本階段只預留介面、不實作；「以設定檔＋control-socket 或 REST Control Agent 整合」留待功能階段決定；「是否取代 Kealight 或抽出共用程式庫」亦留待功能階段再議。

## 理由

- 兩套系統成熟度不同，提早耦合會讓新系統被既有實作細節牽制。
- 詞彙統一能在不共用程式碼的前提下保留認知一致性。
- 在機制未定前強行定案整合方式，風險高於先留下清楚的介面邊界。

## 後果

- 短期內 Kea 相關知識重複存在於兩個系統；未來若證實值得收斂，再以抽出共用 crate 的方式合併。
- asset-nest 的 Kea 模組必須以使用案例定義介面（列出／新增／修改／刪除保留、重載），讓兩種機制皆可替換實作。
