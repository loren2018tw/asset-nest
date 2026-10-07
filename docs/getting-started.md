# 快速開始

asset-nest 是 IT 資產整合管理系統，功能涵蓋：

- **資產與借還**：資產清單、CSV 匯入／匯出、借出／歸還紀錄。
- **網段與 IP**：IP 指派、DHCPv4 保留、位址池、排除範圍與衝突標記；網段匯出／匯入。
- **Kea DHCPv4 整合(預設停用監聽)**：保留與網段層設定（位址池、gateway）單向同步；唯讀的租約清單與系統狀態頁。
- **IP 觀測**：在各網段安裝觀測代理（主動 ARP 掃描＋被動監聽），伺服器彙整呈現，含「網段外觀測」。
- **單一帳號登入**。

以下指令可直接複製；詳細選項與開發說明見 [`README.md`](../README.md)。

## 一鍵安裝 asset-nust 系統

### 1. 安裝

在全新 Ubuntu 24.04／26.04（需 root、需網路）執行，執行完會安裝本系統以及 nginx webserver 和 kea dhcp server 3.2 **(預設關閉監聽 dhcp 請求，以免影響現有的環境中的 dhcp)**：

```sh
curl -fsSL https://raw.githubusercontent.com/loren2018tw/asset-nest/main/deploy/install.sh | sudo bash
```

完成後開啟 `http://<主機>/`（`<主機>` 換成安裝主機的 IP 或名稱），帳號 `admin`；密碼於安裝主機查詢：

```sh
sudo grep '^AUTH_' /etc/asset-nest/asset-nest.env
```

這一步驟完成，就可以登入系統，使用「資產管理」以及「ip管理」綁訂。

### 2. 升級系統

在安裝主機重跑前面的一鍵安裝指令即完成升級（資料庫、設定與 Kea 憑證保留）：


## 3. 一鍵安裝觀測代理（與系統同機）

觀測代理是在背景被動及主動偵測 ip 活動，並回傳資料給系統後台，因為被動偵測功能無法跨 L2 網域，所以必須在要觀測的每個網域，安裝一台 agent 用來收集網路活動。
系統安裝的設備可以時安裝 agent，先觀測同網域的活動。
**這個 agent 可以用來發現哪個 ip 最近上線時間，哪個 ip 被佔用、哪個 ip不在網段中，哪個 mac address 用過哪些 ip。**

### 一鍵安裝觀測代理（與系統同機）
在系統主機執行；認證碼直接讀後端環境檔，`--subnet` 填本機所在網段（假設為 `192.168.1.0/24`）：

```sh
curl -fsSL https://raw.githubusercontent.com/loren2018tw/asset-nest/main/deploy/agent-install.sh | sudo bash -s -- \
  --server-url http://127.0.0.1:8080 \
  --auth-code-file /etc/asset-nest/asset-nest.env \
  --subnet 192.168.1.0/24
```

重跑同一行指令即完成代理更新。安裝後於系統「Kea → 系統狀態」頁的「觀測代理」表可見。

### 一鍵安裝觀測代理（其他網段主機）

以系統主機 `192.168.1.10`、要觀測的另一個網段 `192.168.2.0/24` 為例。

先在系統主機取得代理認證碼：

```sh
sudo grep '^AGENT_AUTH_CODE=' /etc/asset-nest/asset-nest.env
```

再到要觀測網段的主機執行（`--auth-code` 填上一步取得的認證碼）：

```sh
curl -fsSL https://raw.githubusercontent.com/loren2018tw/asset-nest/main/deploy/agent-install.sh | sudo bash -s -- \
  --server-url http://192.168.1.10 \
  --auth-code '<認證碼>' \
  --subnet 192.168.2.0/24
```

## 系統設定

系統登入帳密以以及遠端 agent 認證碼，都在 /etc/asset-nest/asset-nest.env
編輯後，重啟系統就會生效
```bash
systemctl restart asset-nest
```
## Kea dhcp 整合
本系統可以跟 kea dhcp V4 系統整合，ip管理功能可以直接同步到 kea dhcp v4伺服器，無須維護兩份 ip 紀錄。

相關同步功能，安裝後都可以正常使用，如果確認要轉換到本機的 kea dhcp server，只要先停用其他 dhcp server ，然後編輯 /etc/kea/kea-dhcp4.conf 這個檔案，原本 interfaces 設定是 [] 空陣列，填入要回應 dhcp 請求的網路界面(通常是 eth0)

```
"interfaces": [ "eth0" ],
```

然後重啟 kea dhcp server v4 服務即可。

```bash
systemctl restart isc-kea-dhcp4-server.service
```