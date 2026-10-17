# 02: 未回覆目標同輪重試

**What to build:** 主掃一輪後，對未回覆的位址以相同節流與收訊再送一輪（`RETRY_ROUNDS = 1`），結果合併；`checked` 不因重試改變（仍為全部主掃目標）。場域實測同速重試可再收 +16/1000 目標，增益屬瞬時遺失回收。

**Blocked by:** 01

**Status:** ready-for-agent

- [ ] `unanswered(targets, seen)` 純函式（保序、排除已回覆）
- [ ] 重試輪沿用同一節流與 `drain_window`；`seen` 合併後排序、每址第一筆
- [ ] 單元測試：`unanswered`（保序、全未回、全已回、空輸入）
- [ ] 真機 `#[ignore]`（`live_probe`）不需改；確認 `checked` 語意不變

## Comments

（待補）
