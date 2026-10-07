# 03: 文件＋真機驗證——建立缺少網段的流程

**What to build:** `README.md`、`deploy/install.sh` 安裝後提示改為 Kea-first 與 asset-nest-first 並存；`.scratch/kea-subnet-sync/spec.md` 加指向本 feature 的註記；本 spec「實作記錄」彙整三票。真機（`10.1.0.2`）：以 RFC 5737 測試段＋未用 `id` 走完整同步建立 → 驗證（含保留同回合推送、`subnet4-add` 的錯誤訊息形狀）→ `subnet4-del` 清理 → `config-write`（結束後 Kea 無殘留；清理方式於本票決定：測試直送 HTTP 或 client 增方法）。詳見 `.scratch/kea-subnet-create/spec.md`、`docs/adr/0023`。

**Blocked by:** 01、02（真機段需 Kea 可達與使用者同意）

**Status:** ready-for-agent

- [ ] README／install.sh 提示：Kea-first 與 asset-nest-first 並存
- [ ] 舊 spec（`.scratch/kea-subnet-sync/spec.md`）註記指向本 feature
- [ ] 真機建立／驗證／清理（含 `subnet4-del`＋`config-write`）並記錄
- [ ] spec 實作記錄；票檔 Comments＋commit（不 push）
