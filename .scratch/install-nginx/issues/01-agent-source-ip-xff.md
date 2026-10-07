# 01 — 後端：代理入庫來源 IP 採 XFF（loopback 信任）

Status: done
Blocked by: —

## 目標

實作 `spec §4`：`api/agents.rs` 的 heartbeat／observations 以 `peer::resolve_peer_ip` 判定來源 IP，與 peer-mac 同一信任模型（loopback 採 XFF 第一段、否則用連線來源）。

## 範圍

- `backend/src/api/agents.rs`：
  - `heartbeat`、`observations` 兩 handler 的 `source_ip` 改為
    `crate::peer::resolve_peer_ip(remote.ip(), headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()))`。
  - 註解更新：說明信任模型（比照 `peer.rs`；ADR-0019 修訂、ADR-0022）。
- 測試（`backend/tests/agent_ingest.rs`；沿用既有 `send` helper，注入 `ConnectInfo`）：
  - loopback（`127.0.0.1`）＋`X-Forwarded-For: 203.0.113.7, 10.0.0.1` → 心跳與觀測回報後，代理清單／被拒回報的 `source_ip` 為 `203.0.113.7`（取第一段）。
  - loopback 無 XFF → `127.0.0.1`。
  - 非 loopback（既有 `REMOTE=203.0.113.9`）＋偽造 XFF → 仍為 `203.0.113.9`（既有測試不回歸；如需明確覆蓋可加一筆帶 XFF 的斷言）。
- 既有 `agent_status.rs`／`agent_ingest.rs` 測試維持全綠。

## 驗收

- `cargo test --manifest-path backend/Cargo.toml --test agent_ingest --test agent_status` 全綠；`pnpm test` 全綠；`cargo fmt --check`。

## 注意

- 不改 `X-Auth-Code` 認證邏輯；不動 `peer.rs` 既有函式（已是 `pub`）。
- ADR-0019 修訂註記已於設計階段寫入，不需再改文件。
- 不要 `git commit`；不要動 `.scratch/` 內其他票。

## Comments

- 2026-10-07 完成：
  - `backend/src/api/agents.rs`：`heartbeat`／`observations` 的 `source_ip` 改為
    `crate::peer::resolve_peer_ip(remote.ip(), headers.get("x-forwarded-for")…)`；
    模組註解同步改為新信任模型（loopback 採 XFF 第一段、否則連線來源；ADR-0022、ADR-0019 修訂）。
  - `backend/tests/agent_ingest.rs`：`send` 拆為 `send`（固定 `REMOTE`）＋ `send_from`
    （可注入連線來源與 `X-Forwarded-For`）；新增三案例：
    `loopback_source_takes_first_xff_segment`（loopback＋`203.0.113.7, 10.0.0.1` → 心跳／觀測／被拒回報
    `source_ip` 均為 `203.0.113.7`）、`loopback_without_xff_uses_connection_source`（→ `127.0.0.1`）、
    `non_loopback_ignores_forged_forwarded_for`（`203.0.113.9`＋偽造 XFF → 仍 `203.0.113.9`）。
  - 指令結果：`cargo test … --test agent_ingest --test agent_status` 全綠（14＋8 passed）；
    `pnpm test` 全綠（exit=0）；`cargo fmt --check` 通過。
  - 偏離：無。未改 `peer.rs`、認證邏輯、`agent_status.rs`；未 `git commit`。
