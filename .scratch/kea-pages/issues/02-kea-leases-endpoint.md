# 02: 後端 Kea 租約端點

**What to build:** `kea::http::Client` 新增 `lease4_get_all()`（解析 ip-address／hw-address／hostname／subnet-id／cltt／valid-lft／state；`result` 3＝空；state 正規化：0／1／2／3 → default／declined／expired／released、文字原樣）與 `GET /api/v1/kea/leases`：未設定 400（訊息含 `KEA_API_URL`）、命令失敗 502 `kea_error`；回租約清單，`expires_at = cltt + valid_lft`（ISO 8601 UTC），以本地 `kea_subnet_id` 對應 `subnet_cidr`／`subnet_name`（無對應 null）。補 stub 測試與真機 `#[ignore]` 測試（唯讀）。詳見 `.scratch/kea-pages/spec.md`。

**Blocked by:** 01（同一模組檔案，避免衝突）

**Status:** ready-for-agent

- [ ] 未設定 400；stub 命令失敗 502 `kea_error`
- [ ] 0 筆（result 3）回 `{"leases":[]}`
- [ ] `expires_at` 計算與缺欄位 null；state 數字／文字正規化
- [ ] 受管網段對應 CIDR／名稱；無對應 null
- [ ] stub 測試（happy path／未設定／失敗／0 筆）
- [ ] 真機測試（`#[ignore]`）：唯讀實測 `lease4-get-all` 欄位與 0 筆 result

## Comments
