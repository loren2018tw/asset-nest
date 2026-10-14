# 05: 文件：ADR-0027 與詞彙修訂

**What to build:** 記錄本次同步完整性決策（見 `.scratch/kea-sync-completeness/spec.md`）。

**Blocked by:** None

**Status:** done

- [x] `docs/adr/0027-*`：即時推送擴及刪除與 MAC 編輯、匯入提示、網段層維持只增不刪＋警示、錯誤路徑順修
- [x] ADR-0011 第 13 點與 ADR-0026 另案註記加「後續修訂」指標
- [x] `GLOSSARY.md`「推送」定義修訂（涵蓋刪除與 MAC 編輯）
- [x] `README.md`／`docs/getting-started.md` Kea 整合敘述補述

## Comments

實作完成（`9c57558` ADR 與詞彙、`f541296` README／getting-started）。

- 新增 `docs/adr/0027-kea-push-covers-deletes-and-mac-edit.md`。
- ADR-0011「單筆推送」與 ADR-0026「另案」各加「後續修訂（2026-10-14）」引註。
- GLOSSARY「推送」：加入改 hostname／移轉／資產或介面刪除／介面 MAC 變更，指向 ADR-0027。
