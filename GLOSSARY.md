# GLOSSARY.md — asset-nest 領域詞彙

「IT 資產整合管理系統」（asset-nest）的領域詞彙表：只定義名詞與其精確意義，不含實作細節。程式中的型別與欄位以英文名稱為準。

## 核心實體

**Asset（資產）**
被追蹤的 IT 設備（伺服器、網路設備、使用者電腦、印表機等）。資產可以尚無網路介面；純周邊（如螢幕）目前不算 Asset。

**Interface（網路介面）**
Asset 的網路介面，可有 MAC 位址（可空）。MAC 為空的 Interface 代表以手動方式設定 IP 的介面，不可有 Reservation。一台 Asset 可有 0 到多個 Interface；一個 Interface 屬於唯一一台 Asset。

**Subnet（子網段）**
一段連續的 IP 位址範圍（CIDR），位址族為 IPv4 或 IPv6。一個 Subnet 包含多個 IpAddress；一個 IpAddress 只屬於唯一 Subnet。單一 Subnet 為單一地址族，雙棧環境以兩筆 Subnet 表示。DHCP 位址池與 Reservation 本階段僅支援 IPv4。可設選填 gateway；gateway 僅為標記，該位址仍可被指派。

**IpAddress（IP 位址）**
Subnet 中的一個位址，是獨立實體（不是 Asset 的附屬欄位）。用途為下列之一：可用（available）、手動設定（static）、DHCPv4 保留（reservation）、排除（excluded）。是否落在 DHCP 位址池內由 Subnet 的池範圍推導；池內位址不可指派。IPv6 位址採登錄制（僅已指派者存在），不枚舉空閒位址。

**Assignment（指派）**
IpAddress 與 Interface 的對應關係（見 ADR-0005）。同一 Interface 在同一 Subnet 至多一個 IpAddress；跨 Subnet（含 v4、v6 並存）可各有一個。UI 由 Interface 推導所屬 Asset 顯示描述與位置。僅記目前指派，不留歷程；換介面或換 IP 即為取消後重新指派。
_Avoid_: 綁定

**Reservation（保留位址）**
「Subnet 內某個 IpAddress 指派給某個帶 MAC 的 Interface（＋可選 hostname）」的固定對應，供 Kea DHCPv4 據以固定配發。asset-nest 是其真實來源（見 ADR-0002）。

**Lease（租約）**
Kea 動態配發的結果記錄。真實來源是 Kea；asset-nest 僅唯讀檢視。

**DHCP 位址池（pool）**
Subnet 內保留給 Kea 動態配發的位址範圍，可多段；本階段僅 IPv4。池內位址不可指派給 Interface。

**排除範圍（excluded range）**
Subnet 內不可指派給 Interface 的位址範圍，供其他機制使用（如 NAT 對外位址）；可多段。與 Reservation 不同：不綁定任何 Interface，且不得與 DHCP 位址池重疊；本階段僅 IPv4。
_Avoid_: 保留區

## 欄位詞彙

**位置（location）**
Asset 的所在位置。自由文字欄位，輸入時提供既有值建議；不是獨立實體。

**設備序號（device serial number）**
設備原廠序號，選填。單稱「序號」通常指資料庫主鍵、不出現在介面，勿與本詞混用。
_Avoid_: 序號

## 狀態詞彙

**可用（available）**：未被指派、無 Reservation、未被租用，且不在 DHCP 位址池與排除範圍內的位址。
**手動（static）**：已指派給 Interface、由人工在設備端設定的位址；不經 Kea。
**保留（reserved）**：存在對應 Reservation 的位址。
**動態（dynamic）**：目前由 Lease 佔用的位址。
**池內（in-pool）**：落在 DHCP 位址池範圍內的位址；不可指派。
**排除（excluded）**：落在排除範圍內的位址；不可指派。
**衝突（conflict）**：資料互相矛盾的情形（見下方衝突詞彙）。

衝突是標記，不是狀態；以標記呈現，不阻擋儲存與編輯。結構性規則（如網段重疊、同網段重複指派）則直接阻擋，兩層分界見 ADR-0006。

## 衝突詞彙

沿用 Kealight 已定案者：

- **DuplicateHwAddress**：同一 MAC 在同一 Subnet 出現多筆保留。
- **IpInUse**：指派的 IP 已被其他保留佔用。
- **IpInPool**：指派的 IP 落在動態配發池內（Kea 允許此用法，本系統仍沿用 Kealight 的檢出定義）。
- **IpOutOfSubnet**：指派的 IP 不在該 Subnet 範圍內。

觀測衍生（新增，見 ADR-0014）：

- **ObservedMacMismatch**：已指派的位址被觀測到由非宣告 MAC 使用。
- **ObservedOnUnassigned**：未指派、非池內且非排除範圍的位址被觀測到有主；UI 顯示為「非法佔用 IP」。

排除範圍衍生（新增）：

- **IpInExcludedRange**：已指派的 IP 落在排除範圍內。

## 同步詞彙

**真實來源（source of truth）**：該類資料以哪一方為準。
**受管網段（managed subnet）**：已設定 kea_subnet_id 的 IPv4 網段；其保留、位址池與 gateway 單向同步到 Kea，Kea 端尚無該網段時由完整同步建立（不反向刪除）。未受管網段不與 Kea 互動。
**推送（push）**：asset-nest 將保留寫入 Kea 的單向動作；指派／改用途／取消指派時即時推送單筆，失敗不阻擋本地儲存（見 ADR-0011）。
**完整同步（full sync）**：以 asset-nest 為準，一次對齊受管網段的 Kea 保留與網段層設定（位址池、gateway；新增／更新／刪除），並建立 Kea 端缺少的受管網段；先產生唯讀計畫再確認套用（見 ADR-0011、ADR-0013、ADR-0023）。
**網段層設定（subnet settings）**：受管網段在 Kea 端屬網段本身的設定——DHCP 位址池（`pools`）與 gateway（`routers` option）；由完整同步對齊（見 ADR-0013）。
**對帳（reconcile）**：由 Kea 讀回保留與租約、比對差異的唯讀動作；完整同步的計畫階段即為保留對帳（租約對帳後續階段）。

明確非目標：**雙向同步**。

## 觀測詞彙

**觀測（observation）**
對某個位址在某一時刻所見狀態的記錄；由觀測來源產生、唯讀保存，與宣告（Assignment、Reservation）分離，不修改宣告。

**觀測代理（observation agent）**
安裝在某個 L2 網段內、執行觀測並將觀測回報給 asset-nest 的獨立程式；一個要觀測的網段由一個代理負責。
_Avoid_: 遠端 agent、探針、collector

**觀測來源（observation source）**
產生觀測的手段：觀測代理的主動 ARP 探測、觀測代理的持續被動 ARP 監聽、Kea 租約。

**最後可見（last seen）**
某位址最後一次被任一觀測來源實際看到的時間；已指派但從未看到顯示「從未上線」。
_Avoid_: 上線、在線

**涵蓋（coverage）**
某網段由一個健康、近期有回報的觀測代理負責的狀態；未被涵蓋的網段顯示「未觀測」。

**未觀測（not observed）**
位址所屬網段未被涵蓋（尚未安裝觀測代理，或代理失聯）；顯示為「未觀測」，與「從未上線」不同。

**週期掃描（sweep）**
觀測代理對所屬網段全部位址的週期主動探測；用於存活驗證，並發現未指派卻有主、未登錄 MAC。

**持續被動監聽（continuous passive listening）**
觀測代理不發送任何封包、持續監聽 ARP；用於發現網段外的位址與未登錄設備。

**網段外觀測（out-of-subnet observation）**
持續被動監聽到的、位址落在所屬網段 CIDR 外、歸屬於該網段的觀測；UI 顯示於「網段外觀測」清單。

**未知裝置（unknown device）**
被觀測到、但不對應任何 Interface 的 MAC；為觀測值，不是實體。

## 匯入與匯出詞彙

**匯入（import）**
依範本 CSV 批次新增資產（含其 `eth0` 介面與指派）的動作；只新增、不更新，有結構錯誤即整批不匯入（兩層見 ADR-0006）。

**範本（template）**
定義匯入 CSV 欄位與格式的檔案，供下載後以 Excel 整理資料；格式見 ADR-0008。

**問題列報告（issue report）**
匯入預覽中錯誤與警示列的 CSV 下載檔：原列號、原始內容與原因。

**匯出（export）**
由既有資料產生 CSV 的動作：資產匯出採匯入範本 14 欄並依清單現行搜尋／篩選／排序；網段匯出為全部網段設定，供空庫重建匯入使用。

## 借還詞彙

**借出（lend out）**
將 Asset 暫時交給借用人使用的動作；執行即產生一筆借出紀錄。借出時間一律記當下。
_Avoid_: 出借、外借

**出借中（on loan）**
Asset 存在一筆未歸還借出紀錄的狀態。由未歸還紀錄推導，不是 Asset 的儲存欄位。出借中的 Asset 不可再次借出、不可刪除（需先歸還）。

**歸還（return）**
結束出借的動作；在該筆借出紀錄上記歸還時間（當下）。
_Avoid_: 返還、還回

**借出紀錄（Lending）**
一筆「借出到歸還」的完整紀錄：所屬 Asset、借用人、借出時間、歸還時間，及選填的預計歸還日與備註。同一 Asset 可有多筆（期間不重疊），至多一筆未歸還。

**借用人（borrower）**
借出 Asset 的人員名稱。自由文字欄位（同 location），不是獨立實體。
_Avoid_: 借用者、保管人

**預計歸還日（due date）**
借出時約定的歸還日期，選填。

## 認證詞彙

**登入（login）**
以帳號與密碼開始一個工作階段的動作。
_Avoid_: 登錄

**登出（logout）**
結束目前工作階段的動作。

**工作階段（session）**
登入後取得的存取狀態；具固定有效期限，逾期、登出或帳密變更即失效。
_Avoid_: 會話、連線階段

## 邊界詞彙

**Kea**：ISC 的 DHCP 伺服器；本系統的 DHCP 整合（位址池、保留、租約）以 DHCPv4 為對象，IPv6 暫不整合。
**監聽介面（listening interface）**：Kea 伺服器上 DHCPv4 服務綁定的作業系統網路介面（如 `eth0`）；與 Asset 的 Interface（網路介面）不同。空清單代表 Kea 不監聽、不主動服務 DHCP（安裝預設，見 ADR-0012）。
_Avoid_: 界面
**Kealight**：既有的獨立 Kea 設定工具；asset-nest 不依賴它，現階段亦不共用程式碼（見 ADR-0001）。
