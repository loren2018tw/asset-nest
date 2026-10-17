# 03: 佇列容量隨網段推導

**What to build:** 推送佇列容量從固定 10,000 改為依網段 host 數推導：`2 × host 數 ＋ 1,024` 筆，下限 10,000、上限 1,048,576；`runner` 以 `config.subnet` 初始化並在啟動日誌帶容量。滿載仍丟最舊＋警告。場域背景：/16 單輪 65,696 筆對 10,000 容量，每輪丟 60,000 筆且固定丟掉前段（checked 只餘尾巴）。

**Blocked by:** 無

**Status:** ready-for-agent

- [ ] `push::queue_capacity(network)`＋`QUEUE_CAPACITY_MAX`（1,048,576）
- [ ] `runner` 初始化改用 `queue_capacity(&config.subnet)`；啟動日誌附 `queue_capacity`
- [ ] 單元測試：/24→10,000、/16→132,092、/8→上限、/32→下限；既有佇列行為測試不變
- [ ] 滿載語意不變（丟最舊＋警告）；`QUEUE_CAPACITY` 沿用為下限

## Comments

（待補）
