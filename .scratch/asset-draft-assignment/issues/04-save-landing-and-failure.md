# 04: 儲存時落地指派草稿與失敗收尾

**What to build:** `AssetFormDialog.submit()`：儲存時依序建立資產、同步介面、逐筆落地指派草稿；遇錯即停、可重試（見 spec §5.1、§5.2）。

**Blocked by:** 03

**Status:** done

- [x] 預檢：`purpose = reservation` 且其介面草稿無有效 MAC → 阻擋（在任何寫入之前）。
- [x] `syncInterfaces` 後逐筆落地：`assignFromAssetWithTransfer`（共用工具）；同位址＝更新、已指派他處＝確認後移轉。
- [x] 成功：自草稿移除、`notifyKeaSync` 與 warnings 提示照舊；失敗（含移轉被取消）：停止後續、草稿保留、`errorMessage` 顯示（移轉被取消為「位址 X 未移轉，指派尚未建立。」），對話框保持開啟可重試。
- [x] `refreshAssignments(assetId)`：重新載入已落地指派（供部分失敗後呈現）；live 模式的 `onAssignSaved` 改用同一函式。
- [x] 全部落地 → 關閉對話框；部分失敗 → 保持開啟。
- [x] `pnpm typecheck`、`pnpm lint:check`、`pnpm build` 通過。

## Comments

實作完成（主實作 commit `1e5fca0`，`資產指派草稿：介面預填 eth0、草稿式指派與取消處置（前端）`）。

- `submit()`：先 `preflightAssignmentDrafts()`（保留需 MAC，於任何寫入前擋下）；`syncInterfaces()` 後 `landAssignmentDrafts()` 逐筆 `assignFromAssetWithTransfer`；遇錯即停、已成功者自草稿移除、失敗者保留並顯示錯誤；移轉被取消＝「位址 X 未移轉，指派尚未建立。」。
- 落地成功／失敗皆 `emit("saved")` 刷新外層清單（清單「已指派 IP」欄原僅在建立資產後刷新，落地時間較晚會過期）並 `refreshAssignments(assetId)`（改為帶 id 參數；新增模式用 `savedAssetId`）。
- 全部落地才關閉對話框；部分失敗保持開啟可重試（`savedAssetId` 不重建資產；已落地介面／指派跳過）。
- 驗收：`pnpm typecheck`、`pnpm lint:check`、`pnpm build` 全綠。
