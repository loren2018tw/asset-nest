# ADR-0010：以 Kea 內建 HTTP 控制通道整合

- 狀態：已接受
- 日期：2026-10-05

## 背景

Kea 2.7.2 起，DHCPv4／DHCPv6／D2 伺服器內建 HTTP/HTTPS 控制通道（`control-sockets`、`socket-type: http`）；Control Agent（CA）自 Kea 3.0 起進入淘汰、3.1.8 起移除。遠端管理不再需要 CA。

asset-nest 需與遠端 Kea 伺服器（`10.1.0.2:8000`，實測版本 **3.2.1**）整合。ADR-0001 將「設定檔＋control-socket 或 REST Control Agent」的機制選擇留待功能階段；本 ADR 定案。參考實作 Kealight 走「直接編修伺服器設定檔＋control-socket 重載」，其情境為與 Kea 同機。

## 決策

- 整合機制為 **Kea 伺服器內建 HTTP 控制通道**：以 JSON 命令 POST 至 `KEA_API_URL`（單一 URL，含 scheme／host／port）。
- 不使用已移除的 CA；不採設定檔＋unix socket 機制（伺服器為遠端主機，asset-nest 不編修 Kea 設定檔）。
- 第一片垂直切片為 `version-get` 連線檢查（`pnpm test:kea` 真機測試）；保留推送與租約讀取沿用同一通道，`KeaGateway` 介面形狀不變。
- 連線為純 HTTP；Kea 3.2 不支援未認證存取——未認證請求實測被拒（`401`、`WWW-Authenticate: Basic realm="kea-dhcp4"`）。client 以 `KEA_API_USERNAME`／`KEA_API_PASSWORD` 帶 Basic 認證（未設定則不帶 Authorization）。TLS 尚未支援，未來再擴充。

## 理由

- CA 已自 Kea 3.1.8 移除，內建 HTTP 控制通道是官方唯一遠端管理途徑；跟著 Kea 3 對齊，不會踩到移除時程。
- 遠端情境下「檔案＋unix socket」不可行；以命令推送與 ADR-0002（asset-nest 為保留真實來源、單向推送）相符。
- 單一 URL 設定最精簡；變數命名不再沿用易誤導的 CA 詞彙（`KEA_CA_URL`）。

## 後果

- `.env.example` 的 `KEA_CONFIG_PATH`／`KEA_CONTROL_SOCKET`／`KEA_CA_URL` 佔位移除，改為 `KEA_API_URL`（＋認證變數）。
- `backend/src/kea/http.rs` 提供 HTTP client；真機連線測試 `backend/tests/kea_connectivity.rs` 預設忽略。
- 認證採 HTTP Basic（`KEA_API_USERNAME`／`KEA_API_PASSWORD`）；本伺服器為必要，未啟用認證的環境可留空。TLS 尚未支援。
- ADR-0001 中「整合機制留待功能階段決定」由本 ADR 取代，其餘內容不變。
