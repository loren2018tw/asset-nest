# 04: deploy 載入 lease_cmds hook＋真機重驗

**What to build:** `deploy/install.sh` 的 Kea 設定加入 `libdhcp_lease_cmds.so`（與既有 host_cmds 並列；`isc-kea-hooks` 已提供兩者，缺任一即安裝失敗）；真機 10.1.0.2 以最小編輯在 `/etc/kea/kea-dhcp4.conf` 的 `hooks-libraries` 補上 lease_cmds，備份→`kea-dhcp4 -T` 驗證→重啟 `isc-kea-dhcp4-server`；重驗 `lease4-get-all` 真實欄位（`state` 型別、0 筆 `result`）並回寫 spec「待實測定案」；強化票 02 的真機租約測試使其在支援時確實驗證（不再是無聲探測跳過）；`pnpm test:kea` 全綠。背景見 `.scratch/kea-pages/spec.md` 與票 02 Comments。

**Blocked by:** None (can start immediately；票 02 已完成)

**Status:** done

- [x] `deploy/install.sh`：`configure_kea` 同時偵測 host_cmds 與 lease_cmds（找不到即 die）；`render_kea_config` 的 `hooks-libraries` 含兩者；檔頭註解同步
- [x] 既有設定保留路徑（`asset-nest-api.user` 分支）：未含 lease_cmds 時 `warn` 提示（不修改設定）
- [x] 本機驗證：`bash -n`；source 後以假 hook 路徑呼叫 `render_kea_config`，確認兩個 library 條目皆在
- [x] 真機：備份 `/etc/kea/kea-dhcp4.conf`→`hooks-libraries` 加 `libdhcp_lease_cmds.so`（相對名稱，維持檔案權限 `_kea:_kea 0640`）→`kea-dhcp4 -T` 通過→重啟服務→控制通道 `version-get` 仍正常
- [x] `lease4-get-all` 真機實測成功；結果（`state` 型別、0 筆 `result`、欄位）回寫 spec「待實測定案」；若與後端解析牴觸則修後端並補測試
- [x] `backend/tests/kea_connectivity.rs` lease4 測試：支援時改為斷言（result 0／3 與欄位解析合理）；result 2 時維持明確訊息
- [x] `pnpm test:kea` 全綠；既有的 reservation roundtrip 若因真實資料碰撞失敗（測試位址＝broadcast−1 已存在保留），最小幅度強化為「向下找首個未保留位址」，不改變 add→讀回→del 語意
- [x] README／相關註解描述更新；票檔 Comments＋commit（不 push）

## Comments

實作完成（主實作 commit `a075b57`，`04 Kea 檢視：deploy 載入 lease_cmds hook＋真機重驗`）。

- `deploy/install.sh`：新增全域 `KEA_LEASE_HOOK_PATH`；`configure_kea` 比照 host_cmds 以 `/usr/lib/*/kea/hooks/libdhcp_lease_cmds.so` glob 偵測（找不到即 `die`）；`render_kea_config` 的 `hooks-libraries` 改為兩個條目（host_cmds、lease_cmds 皆用變數）；既有設定保留分支（`asset-nest-api.user`）在未含 `libdhcp_lease_cmds` 時 `warn` 提示（不改設定）；檔頭註解同步。本機驗證：`bash -n` 通過；source 後以假路徑呼叫 `render_kea_config`、去註解 JSON 解析確認兩條目（`[{"library": "/x/libdhcp_host_cmds.so"}, {"library": "/x/libdhcp_lease_cmds.so"}]`）。
- 真機（10.1.0.2／kea3，Kea 3.2.1）：抵達時設定檔已由人工編輯器改到一半且語法錯誤（`library` 為陣列／反引號），服務 failed；18:52:25 人工修正為兩個獨立條目後服務恢復 active。本票未再改寫設定；於服務恢復後補備份 `/etc/kea/kea-dhcp4.conf.bak.20261005185249`、驗證 `kea-dhcp4 -T` 通過、權限維持 `_kea:_kea 0640`；journal 見 `LEASE_CMDS_INIT_OK`／`HOOKS_LIBRARY_LOADED libdhcp_lease_cmds.so` 與 host_cmds 並列。
- 真機實測（控制通道唯讀、憑證讀 `/etc/kea/api-{user,password}`）：`version-get` 正常（`result` 0、`text` 3.2.1）；`lease4-get-all` 回 `[ { "arguments": { "leases": [ ] }, "result": 3, "text": "0 IPv4 lease(s) found." } ]`——0 筆時 `result` 3 且 `arguments.leases` 為空陣列，與後端「result 3 → 空清單」解析相符；該機 `interfaces: []` 且 memfile（`/var/lib/kea/kea-leases4.csv` 僅表頭）無動態租約，`state` 型別與逐筆欄位仍無法於真機觀測（解析以 Kea ARM 規格為準、stub 測試覆蓋）。
- 測試：`pnpm test:kea` 4 passed／0 failed（lease4 測試成功路徑 0 筆、無斷言可觸發；roundtrip 以新邏輯由 broadcast−1 往下找到 `140.128.179.253`，避開既有真實保留 `.254`）；`cargo test --manifest-path backend/Cargo.toml` 全綠（99 單元＋131 整合、共 230 passed、4 ignored、0 failed）；`cargo fmt --check` 綠。
- spec：`待實測定案（真機）` 改為 `真機實測結論（Kea 3.2.1）` 並補票 04 實測（`result` 3、`arguments.leases` 空陣列、`state` 仍不可得之理由）；範圍註記 `deploy/` 例外。
- README 無 host_cmds／lease_cmds 相關描述，無須更新（ADR-0012 為歷史決策、內容仍正確，未動）。
- 未 push。
