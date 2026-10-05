# 02: 後端 Kea 租約端點

**What to build:** `kea::http::Client` 新增 `lease4_get_all()`（解析 ip-address／hw-address／hostname／subnet-id／cltt／valid-lft／state；`result` 3＝空；state 正規化：0／1／2／3 → default／declined／expired／released、文字原樣）與 `GET /api/v1/kea/leases`：未設定 400（訊息含 `KEA_API_URL`）、命令失敗 502 `kea_error`；回租約清單，`expires_at = cltt + valid_lft`（ISO 8601 UTC），以本地 `kea_subnet_id` 對應 `subnet_cidr`／`subnet_name`（無對應 null）。補 stub 測試與真機 `#[ignore]` 測試（唯讀）。詳見 `.scratch/kea-pages/spec.md`。

**Blocked by:** 01（同一模組檔案，避免衝突）

**Status:** done

- [x] 未設定 400；stub 命令失敗 502 `kea_error`
- [x] 0 筆（result 3）回 `{"leases":[]}`
- [x] `expires_at` 計算與缺欄位 null；state 數字／文字正規化
- [x] 受管網段對應 CIDR／名稱；無對應 null
- [x] stub 測試（happy path／未設定／失敗／0 筆）
- [x] 真機測試（`#[ignore]`）：唯讀實測 `lease4-get-all` 欄位與 0 筆 result

## Comments

實作完成（commit `bac761a`，`02 Kea 檢視：後端租約端點（client＋API＋stub／真機測試）`）。

- `backend/src/kea/http.rs`：新增 `KeaLease`（ip-address／hw-address／hostname／subnet-id／cltt／valid-lft／state）與 `lease4_get_all()`——送 `lease4-get-all` 不帶 arguments、`result` 3（空）視為空清單、其他 result 走既有 `response_error`；`parse_lease()` 空字串／空白視為未提供（比照 `parse_host`）、數值欄位接受 JSON 數字或數字字串；`normalize_state()` 數字 0／1／2／3 → default／declined／expired／released、文字小寫、未知保留原值。唯讀：不帶 `operation-target`、不呼叫 `config-write`。
- `backend/src/api/kea.rs`：新增 `GET /api/v1/kea/leases`（掛入既有 router）。未設定 `KEA_API_URL` → 400 `validation_error`（`client()` helper 增加 action 參數；同步端點訊息不變、租約端點為「…無法讀取租約」）；命令失敗 → 502 `kea_error`（`ApiError::kea`）。200 回 `{"leases":[...]}`，每筆 `ip_address`／`hw_address`／`hostname`／`subnet_id`／`subnet_cidr`／`subnet_name`／`expires_at`／`state`；`expires_at = cltt + valid_lft` 轉 `%Y-%m-%dT%H:%M:%SZ`（缺欄位或溢位 null）；`subnet_cidr`／`subnet_name` 以本地 `subnets.kea_subnet_id` 對應、無對應 null。
- 測試：`backend/tests/kea_view.rs` stub 新增 `lease4-get-all`（可回 leases 陣列或 result 3）；新增 4 整合測試——happy path（帶名稱的受管網段、`expires_at` 計算、state 數字 0／3 與文字小寫、空字串 hostname null、subnet-id 9 無對應 CIDR／名稱 null、讀取命令不帶 arguments）、未設定 400＋stub 零命令、失敗 502 `kea_error`、result 3 回 `{"leases":[]}`；`kea/http.rs` 新增 3 單元測試（parse／缺欄位／state 正規化）。
- 真機實測（`pnpm test:kea`，Kea 3.2.1、`http://10.1.0.2:8000`）：**該機未載入 `lease_cmds` hook**（raw `config-get` 的 `hooks-libraries` 僅 `libdhcp_host_cmds.so`），`lease4-get-all` 回 `result` 2「'lease4-get-all' command not supported.」——欄位與 0 筆 `result` 無法於該機實測，照實記錄。真機唯讀測試 `lease4_get_all_against_live_server` 對 result 2 印出環境限制（不 panic）、其他錯誤仍 panic；本輪 4 passed／0 failed（version-get／status／roundtrip 亦通過）。首次執行時 roundtrip 曾因抽到 subnet 2 的 `140.128.179.254`（既有真實保留、MAC `84:39:8f:ad:cf:30`）而失敗，屬既有測試抽樣脆性，與本票無關。
- **後續（未於本票處理）**：租約清單要能對真機運作，需 Kea 載入 `libdhcp_lease_cmds.so`（`deploy/install.sh` 目前 `hooks-libraries` 只設定 `host_cmds`）；`deploy/` 依限制未動。spec「待實測定案」已依實測更新。
- 驗收：`cargo fmt --check` 綠；`cargo test --manifest-path backend/Cargo.toml` 全綠（99 單元＋131 整合、共 230 passed、4 ignored、0 failed；票 01 測試無退步）。
- 實作 commit：`bac761a`。
