# 02: 指派對話框草稿模式與共用移轉確認

**What to build:** `AssignIpDialog.vue` 新增「草稿模式」：不呼叫資產／指派 API，只做預檢並以事件回傳指派草稿；外觀與現行一致。同時把「位址已指派他處 → 確認 → `transfer: true` 重試」抽成共用工具，供草稿落地與現行指派共用（見 spec §4.2、§4.3）。

**Blocked by:** 無

**Status:** ready-for-agent

- [ ] 新增 `frontend/src/utils/assignTransfer.ts`：`parseAssignedElsewhere`、移轉確認對話框、`assignFromAssetWithTransfer($q, assetId, input)`（成功回 `AssetAssignmentSaved`、使用者取消移轉回 `null`、其他錯誤拋出）；`AssignIpDialog` 改用之，現行行為與文案不變。
- [ ] `AssignIpDialog` props：`asset` 改為可空；新增 `draft?: { subtitle; interfaceName; interfaceMac; existing: { address; subnetId }[]; subnets: SubnetSummary[] | null } | null`。
- [ ] 草稿模式：不 `fetchAsset`、不載入介面與已指派清單；副標與介面欄取自 `draft`；不顯示對話框自身的「已指派 IP」區塊。
- [ ] 草稿模式送出前預檢（spec §4.3）：v4 以 `query_status` 擋 `in_pool`／`excluded`／`out_of_subnet`（無查詢結果時即時補查、查詢失敗擋下）；v6 以 `subnets` 比對 CIDR（清單為 `null` 時略過）；同介面同網段同位址＝更新、不同位址＝擋下（訊息沿用後端文案）。
- [ ] 草稿模式送出：`emit("draft", { address, purpose, hostname, subnetId, subnetLabel })` 後關閉；不呼叫 API、不推送 Kea。
- [ ] 現行（live）模式行為不變。
- [ ] `pnpm typecheck`、`pnpm lint:check`、`pnpm build` 通過。

## Comments
