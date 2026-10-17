# 規格：掃描可靠性（逐筆節流、同輪重試、佇列容量推導）

- 狀態：已定案（2026-10-17）；已實作（2026-10-17，見票 01–03）。
- 詞彙依 `GLOSSARY.md`；決策見 `docs/adr/0028`；場域診斷為本規格 Problem Statement。
- 前置：`.scratch/observation-agent/spec.md`（代理模型與語意不變；本規格修訂掃描送收與佇列容量）。

## Problem Statement

場域實測（10.1.0.3、代理 `kea3-2`、受管網段 10.1.0.0/16）證實三個缺陷，使活著、ping 得到、且同 L2 的設備常態顯示「從未上線」（代表案例 10.1.6.139）：

1. **1000 筆爆量送訊**：`AGENT_SWEEP_RATE_PPS` 被當成批次大小，批內連發（1000 筆 ≈ 8–10ms 廣播風暴）。同一台設備（10.1.6.139）在批次位置 138 會回覆、位置 662 的探測則在鏈路端遺失（tcpdump 確認回覆從未回到代理主機；tcpdump 與 socket 對照排除收訊端問題）。批次實收率實測（1000 目標、10.1.3.239–10.1.7.222）：

   | 送訊方式 | 實收回覆 |
   |---|---|
   | 爆量（8–10ms） | 139–146 筆 |
   | 497 pps | 167 筆 |
   | 332 pps | 175 筆 |
   | 200 pps | 198 筆（含 10.1.6.139） |
   | 100 pps | 229 筆（含） |
   | 50 pps | 234 筆（含） |
   | 極慢（≲15 pps） | 239 筆（含） |

   同速重試對未回覆者可再收 +16 筆；慢速重試再 +28 筆。結論：損失主要取決於**送訊速率**（遠端鏈路被風暴壓垮），節流是主修正、重試是增益。

2. **佇列容量 10,000 對大網段不足**：/16 單輪 `checked＋seen` ≈ 65,696 筆；切批後佇列每輪丟棄 60,000 筆（日誌 `dropped=60000` 逐輪重現），僅存「最後 5,534 筆 checked＋全部 seen」——即 10.1.234.100–255.99。其餘 60,000 筆的 `last_checked_at` 永遠到不了後端（10.1.6.139 連現況列都沒有），且被丟棄區段逐輪固定。

3. **回覆窗閒置被背景雜訊刷新**：平面 L2 上任何背景 ARP 訊框都會重置「距最後一幀」計時，500ms 提早結束永不觸發——每批跑滿 2 秒（實測 2.09s/批），/16 掃描 138 秒幾乎全花在空等。

## Solution

- `AGENT_SWEEP_RATE_PPS` 改為**真正的每秒送訊上限**：逐筆節流送出（間隔 `1 / rate` 秒），送訊期間即時收訊（非阻塞 drain），全部送完後進入收尾回覆窗；批次概念退場。
- 收尾回覆窗的閒置判斷改以**回覆**為準（背景 ARP 不延長）；2 秒硬上限與「全部目標已回應」提早結束不變。
- **同輪重試一次**：主掃後對未回覆位址以相同節流再送一輪。
- **佇列容量隨網段推導**：`2 × host 數 ＋ 1,024` 筆，下限 `10,000`、上限 `1,048,576`；滿載仍丟最舊。
- 預設速率 `1000 → 200`：/16 主掃＋重試 ≈ 11 分鐘留在 900 秒週期內，且 200pps 已涵蓋 10.1.6.139 類設備。

## Implementation Decisions

### 掃描（`agent/src/probe.rs`）

- `probe_targets_platform`：開 socket 後執行「主掃＋至多 `RETRY_ROUNDS = 1` 輪重試」。
- 每輪送訊：逐筆 `send_frame` → `drain_available`（`MSG_DONTWAIT` 非阻塞迴圈）→ 節流 sleep 至下一時槽；時槽以 deadline 制（`frame_interval(rate) = 1s / rate`，最小 1pps），維持長期平均速率。
- 每輪收尾：`drain_window` 以 `reply_window_done(經過、距最後一筆回覆、已回應數、本輪目標數、硬上限 2s、閒置 500ms)` 結束；本輪新收覆才更新「最後一筆回覆」。
- 收集條件不變：opcode 2、sender IP ∈ 本輪目標、每址第一筆；成員判定用 `HashSet`。
- 主掃結束後以 `unanswered(targets, seen)`（保序）取得未回覆者，重試輪只送這些。
- `checked` ＝ 主掃送出的全部目標（不因重試改變）；`seen` 兩輪合併後排序。
- 送訊失敗（`send_frame`）維持整輪 `Err`；drain 非逾時錯誤停止該 drain、保留已收集結果（同現行）。

### 佇列（`agent/src/push.rs`、`agent/src/runner.rs`）

- 新增 `pub fn queue_capacity(network: &Ipv4Net) -> usize`：`(2 × host 數 ＋ 1024).clamp(10_000, 1_048_576)`；並新增 `QUEUE_CAPACITY_MAX`。
- `runner` 以 `config.subnet` 初始化 `PushQueue`，啟動日誌帶 `queue_capacity`。
- 單輪 `checked＋seen ≤ 2 × host 數`，故 /16 一輪完整入列；滿載語意不變（丟最舊＋警告）。
- 不做 checked/seen 優先級丟棄：latest-wins 語意下長時離線保留最新者即正確取捨（見 ADR-0028）。

### 設定與文件

- `config.rs`：`DEFAULT_SWEEP_RATE_PPS = 200`；註解改為「每秒送出上限（逐筆節流）」。
- `deploy/agent-install.sh`：`--rate` 說明與安裝摘要預設值 1000 → 200。
- `README.md`：`--rate` 預設值與週期掃描描述（逐筆節流、同輪重試）更新。

## Testing Decisions

- 單元（`probe.rs`）：`frame_interval`；`unanswered` 保序與排除已回覆；`accept_reply` 條件與每址第一筆（linux）；`reply_window_done` 語意更新為「回覆為準」；空目標不開 socket 不變。
- 單元（`push.rs`）：`queue_capacity`（/24→10,000；/16→132,092；/8→上限；/32→下限）；既有佇列行為測試不變。
- 真機 `#[ignore]`（`live_probe`／`live_passive`）不變（`probe_targets` 介面與語意不變）。
- 驗收：`pnpm test:agent` 全綠；`cargo fmt --check` 綠；後端與前端不受影響。

## Out of Scope

- 自適應速率（依鏈路品質動態調整）；極慢鏈路設備（實測 ≤50pps 才回）以調低 `AGENT_SWEEP_RATE_PPS` 因應。
- 後端入庫與顯示邏輯；涵蓋語意（本環境為單一平面 /16，非跨 VLAN）。
- 被動監聽變更；`seen`／`checked` 優先級丟棄策略（容量推導後正常輪次零丟棄）。
