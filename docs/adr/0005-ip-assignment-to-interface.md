# ADR-0005：IP 指派以 Interface 為對象，Asset 僅為彙總顯示

- 狀態：已接受
- 日期：2026-10-04

## 背景

需求原以「把 IP 指定給某個資產」描述，但 Kea 的固定保留綁定 MAC（介面），且一台 Asset 可有多個 Interface（含 MAC 空白的介面）。若指派只落在 Asset 層級，未來推送 Kea 時將缺少 MAC 對應，也無法回答「是哪張網卡」。

## 決策

- IpAddress 的指派對象是 Interface，不是 Asset；UI 由 Interface 推導所屬 Asset，顯示資產描述與位置。
- 不變條件：同一 Interface 在同一 Subnet 至多一個 IpAddress；跨 Subnet（含 v4、v6 並存）可各有一個。
- MAC 空白的 Interface 只能有手動設定位址，不可有 Reservation。
- 指派僅記目前狀態、不留歷程；換介面或換 IP 即為取消後重新指派。未來若需稽核再另案評估。

## 理由

- Kea 保留需要 MAC；介面層級是唯一能同時承載「保留」與「手動」兩種用途的層。
- 一台資產多介面（有線、無線、管理埠）是常態，資產層指派無法表達網卡。

## 後果

- 所有「以資產查 IP」的需求都經由 Interface 關聯實作。
- 資產刪除、介面刪除對指派與保留的連動規則留在功能階段訂定。
