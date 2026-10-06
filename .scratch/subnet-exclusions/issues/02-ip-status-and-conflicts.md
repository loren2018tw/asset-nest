# 02 — IP 狀態「排除」、篩選與衝突標記

Status: ready-for-agent
Blocked by: 01

## 目標

實作 `spec §5、§6`：IP 清單新狀態 `excluded`、篩選、排序；語意衝突 `IpInExcludedRange`；觀測 `ObservedOnUnassigned` 抑制。

## 範圍

- `backend/src/ips.rs`：
  - `IpStatusFilter::Excluded`（`parse("excluded")`、`as_str()`），`IpFilter` 白名單。
  - 新增排除範圍解析（比照 `parse_pools`）；`V4Context` 帶 `exclusions`；`status_of` 判定：指派優先 → pool → exclusion → available；`entry_v4`、`matches_v4_filters`、`push_sort_key_v4` 隨之調整。
  - `status_rank`：`available(0) → in_pool(1) → excluded(2) → static(3) → reservation(4)`。
  - 觀測篩選 `unassigned_seen`：位址在排除範圍內時不列入。
  - 單元測試：excluded 狀態、`status=excluded` 篩選、available 不含排除、排序 rank、`unassigned_seen` 抑制。
- `backend/src/api/ips.rs`：狀態白名單錯誤訊息改為「狀態篩選須為 available、in_pool、excluded、static 或 reservation」。
- `backend/src/conflicts.rs`：
  - `pub const IP_IN_EXCLUDED_RANGE: &str = "IpInExcludedRange"`；`detect` 對 v4 指派加入偵測（代碼順序：IpOutOfSubnet → IpInPool → IpInExcludedRange → DuplicateHwAddress）；`message_of`：「位址 {address} 落在排除範圍內（僅提示，不阻擋儲存）」。
  - `observed_codes`：`ObservedOnUnassigned` 加上「非排除範圍」條件。
  - 單元測試：偵測、訊息、代碼順序（可與 IpOutOfSubnet 並存）、排除範圍內觀測不標記（比照池內既有測試）。
- `backend/tests/ips.rs`（整合）：
  - `?status=excluded` 回排除列；available 清單不含排除位址。
  - 「先指派、後 PATCH 網段加排除範圍」→ 該列 `status` 仍為指派用途，且 `conflicts` 含 `IpInExcludedRange`。
  - `unassigned_seen` 篩選不含排除位址（沿用該檔既有觀測資料 setup 手法；若門檻過高，至少以單元測試覆蓋並在 PR 說明）。

## 驗收

- `cargo test --manifest-path backend/Cargo.toml --lib ips`、`--lib conflicts`、`--test ips`。
- `pnpm test` 全綠；`cargo fmt --check`。

## 注意

- 不要改指派寫入（票 03）與候選端點（票 04）。
- 不要 `git commit`。
