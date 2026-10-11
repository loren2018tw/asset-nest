# 04: 儲存時落地指派草稿與失敗收尾

**What to build:** `AssetFormDialog.submit()`：儲存時依序建立資產、同步介面、逐筆落地指派草稿；遇錯即停、可重試（見 spec §5.1、§5.2）。

**Blocked by:** 03

**Status:** ready-for-agent

- [ ] 預檢：`purpose = reservation` 且其介面草稿無有效 MAC → 阻擋（在任何寫入之前）。
- [ ] `syncInterfaces` 後逐筆落地：`assignFromAssetWithTransfer`（共用工具）；同位址＝更新、已指派他處＝確認後移轉。
- [ ] 成功：自草稿移除、`notifyKeaSync` 與 warnings 提示照舊；失敗（含移轉被取消）：停止後續、草稿保留、`errorMessage` 顯示（移轉被取消為「位址 X 未移轉，指派尚未建立。」），對話框保持開啟可重試。
- [ ] `refreshAssignments(assetId)`：重新載入已落地指派（供部分失敗後呈現）；live 模式的 `onAssignSaved` 改用同一函式。
- [ ] 全部落地 → 關閉對話框；部分失敗 → 保持開啟。
- [ ] `pnpm typecheck`、`pnpm lint:check`、`pnpm build` 通過。

## Comments
