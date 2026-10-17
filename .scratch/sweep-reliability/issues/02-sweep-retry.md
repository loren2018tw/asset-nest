# 02: 未回覆目標同輪重試

**What to build:** 主掃一輪後，對未回覆的位址以相同節流與收訊再送一輪（`RETRY_ROUNDS = 1`），結果合併；`checked` 不因重試改變（仍為全部主掃目標）。場域實測同速重試可再收 +16/1000 目標，增益屬瞬時遺失回收。

**Blocked by:** 01

**Status:** done

- [x] `unanswered(targets, seen)` 純函式（保序、排除已回覆）
- [x] 重試輪沿用同一節流與 `drain_window`；`seen` 合併後排序、每址第一筆
- [x] 單元測試：`unanswered`（保序、全未回、全已回、空輸入）
- [x] 真機 `#[ignore]`（`live_probe`）不需改；確認 `checked` 語意不變

## Comments

實作於 `cb8d72c`（與票 01 同 commit）：

- `unanswered`＋`RETRY_ROUNDS = 1`；`probe_targets_platform` 主掃後以未回覆者為次輪目標，同節流送收；`checked` 維持全部主掃目標、`seen` 兩輪合併排序。
- 測試：`unanswered_keeps_order_and_excludes_seen`；`live_probe` 介面與斷言不變（`checked == targets`）。
- **場域驗證**：/16、200pps 主掃＋重試共約 10.9 分鐘；整體 `seen=239`（含 10.1.6.139——修正前的固定漏失案例）。
