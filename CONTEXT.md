# CONTEXT.md — asset-nest 領域詞彙

「IT 資產整合管理系統」（asset-nest）的領域詞彙表：只定義名詞與其精確意義，不含實作細節。程式中的型別與欄位以英文名稱為準。

## 核心實體

**Asset（資產）**
被追蹤的 IT 設備（伺服器、網路設備、使用者電腦、印表機等）。資產可以尚無網路介面；純周邊（如螢幕）目前不算 Asset。

**Interface（網路介面）**
Asset 的網路介面，以 MAC 位址為識別核心。一台 Asset 可有 0 到多個 Interface；一個 Interface 屬於唯一一台 Asset。

**Subnet（子網段）**
IP 位址所屬的網段範圍。一個 Subnet 包含多個 IpAddress；一個 IpAddress 只屬於唯一 Subnet。

**IpAddress（IP 位址）**
Subnet 中的一個 IPv4 位址，是獨立實體（不是 Asset 的附屬欄位）。與 Interface 為多對多關係：一個 Interface 可隨時間被指派不同 IpAddress，一個 IpAddress 也可先後被不同 Interface 使用。

**Reservation（保留位址）**
「Subnet 內某個 IpAddress＋某個 Interface 的 MAC（＋可選 hostname）」的固定對應，供 Kea DHCP 據以固定配發。asset-nest 是其真實來源（見 ADR-0002）。

**Lease（租約）**
Kea 動態配發的結果記錄。真實來源是 Kea；asset-nest 僅唯讀檢視。

## 狀態詞彙

**可用（available）**：未被保留、未被租用，且不在 Kea 動態配發池內的位址。
**保留（reserved）**：存在對應 Reservation 的位址。
**動態（dynamic）**：目前由 Lease 佔用的位址。
**衝突（conflict）**：資料互相矛盾的情形（見下方 Conflict 詞彙）。

（狀態的判定細節屬功能階段，本階段僅定詞。）

## 衝突詞彙（沿用 Kealight 已定案者）

以下判定詞彙沿用既有工具 Kealight 的定義，落地於本系統的時機留待功能階段：

- **DuplicateHwAddress**：同一 MAC 在同一 Subnet 出現多筆保留。
- **IpInUse**：指派的 IP 已被其他保留佔用。
- **IpInPool**：指派的 IP 落在動態配發池內。
- **IpOutOfSubnet**：指派的 IP 不在該 Subnet 範圍內。

## 同步詞彙

**真實來源（source of truth）**：該類資料以哪一方為準。
**推送（push）**：asset-nest 將保留寫入 Kea 的單向動作。
**對帳（reconcile）**：由 Kea 讀回保留與租約、比對差異的唯讀動作（第二階段）。

明確非目標：**雙向同步**。

## 邊界詞彙

**Kea**：ISC 的 DHCP 伺服器；本系統先以 DHCPv4 為對象。
**Kealight**：既有的獨立 Kea 設定工具；asset-nest 不依賴它，現階段亦不共用程式碼（見 ADR-0001）。
