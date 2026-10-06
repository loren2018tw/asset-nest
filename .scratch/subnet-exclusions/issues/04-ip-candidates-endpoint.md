# 04 — 指派候選端點 `GET /api/v1/ip-candidates`

Status: ready-for-agent
Blocked by: 01

## 目標

實作 `spec §8`：跨網段前綴搜尋可用位址＋查詢位址狀態。

## 範圍

- 新增 `backend/src/ip_candidates.rs`（領域＋回應型別）：
  - `Candidate { address, subnet_id, subnet_cidr, subnet_name: Option<String> }`、`QueryStatus { address, status, subnet_id: Option<i64>, subnet_cidr: Option<String>, subnet_name: Option<String> }`、`Candidates { items, query_status: Option<QueryStatus> }`（皆 `Serialize`）。
  - 前綴解析（spec §8）：完整 octet 精確、最後一段前綴段、結尾點；至少 2 個完整 octet，否則 400 `invalid_query`（field `q`、「查詢前綴至少須包含兩個完整 octet（例：10.0.）」）；格式錯誤（非數字、>4 段、空段、完整 octet >255）→ 400。
  - `pub async fn find(db: &SqlitePool, q: &str, limit: usize) -> Result<Candidates, ApiError>`：
    - `items`：v4 host 且可用（無指派、非池內、非排除範圍）。以完整 octet 固定前段；前綴段以值集合 M＝{o∈0..=255: `o.to_string().starts_with(partial)`}、其後為自由 octet 0..=255；依數值升冪產生位址，檢查所屬網段（CIDR 索引）與可用性，命中即收、達 `limit` 停止。匹配空間 ≤ 65,536（spec §8 效能界線），不得無界掃描。
    - `query_status`：`q` 為完整 v4 位址才有值（否則 `None`）。判定順序（spec §8）：無所屬網段 → `out_of_subnet`；有指派 → `static`／`reservation`；非 host → `out_of_subnet`；池內 → `in_pool`；排除 → `excluded`；否則 `available`。`subnet_*`：落在某網段 CIDR 內時帶值，否則 null。
    - 預載 `subnets::list_full`；指派以網段為單位快取（`assignments::list_for_subnet`），避免逐位址查詢。
- 新增 `backend/src/api/ip_candidates.rs`：`GET /ip-candidates`，Query `{ q: Option<String>, limit: Option<i64> }`；`limit` 預設 20、夾在 `1..=50`；回 `Json<Candidates>`。
- 註冊路由：`backend/src/lib.rs` 加 `pub mod ip_candidates;`；`backend/src/api/mod.rs` 加 `mod ip_candidates;` 與 `.merge(super::ip_candidates::router())`。
- 新增 `backend/tests/ip_candidates.rs`（測試骨架比照 `tests/subnets.rs`；資產／介面／指派 setup 比照 `tests/assignments.rs`）：
  - `q=10.0.1.` 只含 10.0.1.x（**不含** 10.0.10.5）；`q=10.0.1` 依前綴語意同時含 10.0.1.x 與 10.0.10.x。
  - 已指派、池內、排除位址不出現在 `items`；network/broadcast 不出現。
  - `limit` 生效、預設 20、上限 50（>50 夾住）。
  - `query_status` 六態（available／in_pool／excluded／static／reservation／out_of_subnet）。
  - 400 邊界：`q` 缺、`q=10`（不足 2 完整 octet）、`q=10.0.1.2.3`、`q=abc`；錯誤 `error=validation_error`、`field=q`。

## 驗收

- `cargo test --manifest-path backend/Cargo.toml --test ip_candidates`。
- `pnpm test` 全綠；`cargo fmt --check`。

## 注意

- 不修改既有 `/subnets/{id}/ips` 行為（票 02）。
- 回應不含 total；`items` 無結果為空陣列。
- 不要 `git commit`。
