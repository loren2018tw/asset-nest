# 03: Kea 租約來源

**What to build:** 快速掃描把 Kea 目前有效的 DHCP 租約納入：租約位址成為探測目標，`cltt` 記為「最後可見」（來源 `kea_lease`），因此不回應 ARP 的 DHCP 裝置（如 Windows）仍有活動訊號；IP 清單的來源提示可分辨 ARP 與租約。

**Blocked by:** 02 快速掃描與「最後可見」欄（核心）

**Status:** done

- [x] 快速掃描目標集合＝已指派位址 ∪ 有效租約位址（`state=default`；以 `kea_subnet_id` 對應本機網段）
- [x] 租約以 `cltt` 記為最後可見、來源 `kea_lease`；非 `default` 狀態不記
- [x] 同一位址的 ARP 與租約訊號取最近者；Kea 未設定時行為不變
- [x] 以 stub Kea 模式整合測試；`pnpm lint:check`／`pnpm typecheck` 全綠

## Comments

實作完成（主實作 commit `ebecb66`，`03 IP 觀測：Kea 租約來源（cltt 記為最後可見）`；未新增 migration）。

- `backend/src/observation.rs`：`run_quick` 新增 `kea: Option<&kea::http::Client>` 參數；目標＝已指派位址 ∪ 有效租約位址（`filter_lease_signals`：`state == Some("default")` ∧ `subnet_id == kea_subnet_id` ∧ `ip_address` 可解析 IPv4）。未設定 Kea 或 `kea_subnet_id` 為 `None` 時不呼叫 Kea；`lease4_get_all` 失敗記 `warn` 後僅掃已指派位址，不讓掃描失敗。
- 租約寫入走同一 `record_seen` 路徑（`mac` 參數改為 `Option<&str>`）：時間 `timestamp(cltt)`、來源 `kea_lease`；同 MAC 不重複事件、換 MAC 寫 `mac_changed`（事件記寫入來源）、缺 `hw-address` 不寫事件且保留既有 `last_seen_mac`。租約先寫、ARP 後寫，最近者勝（同秒時 ARP 直接證據勝出）。所有目標（含租約）upsert `last_checked_at`；`seen`＝ARP 回應 ∪ 具 `cltt` 有效租約之相異位址數。
- `backend/src/api/subnets.rs`：sweeps handler 以 `state.kea.as_ref()` 帶入（`KEA_API_URL` 未設定時為 `None`，行為同票 02）。
- 邊界定義：缺 `cltt` 的 `default` 租約仍納入探測目標，但不記「最後可見」（無時間可記）；`valid_lft` 不額外檢查，語意以 Kea `state` 為準（見 ADR-0015）。
- 測試：新增 `tests/observation_lease.rs` 4 項（stub Kea＋stub 探測邊界）：租約位址併入目標（非 default／不同 subnet／非 IPv4／缺 state 不記）、`cltt` 記 `kea_lease` 與 MAC 正規化（缺 MAC 無事件）、latest-wins 雙向（新 ARP 勝舊租約；新租約勝舊 ARP；換 MAC 寫 `mac_changed` 來源 `kea_lease`；以固定 `now` 直呼服務）、未設定 Kea 不發命令不留租約列、Kea 失敗仍成功且 ARP 照常；單元測試 `filter_lease_signals`（狀態／subnet／IPv4／MAC／cltt 選取）。
- 驗收：`cargo fmt --check` 綠；`cargo test` 264 passed／0 failed／6 ignored；`pnpm lint:check`、`pnpm --filter frontend typecheck` 綠（前端未動）。
- 票 04 介接：`run_quick(pool: &SqlitePool, prober: Arc<dyn Prober + Send + Sync>, kea: Option<&kea::http::Client>, subnet: &Subnet, now: DateTime<Utc>) -> Result<SweepReport, ApiError>`。排程器 due 判斷目前無持久化「上次快速掃描」欄位；建議以排程器記憶體（啟動後視為 due）或 `SELECT MAX(last_checked_at) FROM ip_presence WHERE subnet_id = ?` 推得，惟後者對「無任何目標」的網段永遠 due（每 60 秒重跑），需一併處理；Kea client 以 `state.kea.as_ref()` 直通即可。
