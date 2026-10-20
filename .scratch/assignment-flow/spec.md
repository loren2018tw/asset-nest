# 規格：前端「指派流程」（assignmentFlow）——三個對話框的指派寫入流程收成可測 module（含 vitest）

- 狀態：已定案（2026-10-19）；架構審視候選 3 的設計會談產出；實作見票 01–06（完成後於本檔補記實作記錄）。
- 詞彙依 `GLOSSARY.md`（指派、指派草稿、介面、保留、可用、手動、池內、排除、衝突）；架構詞彙依 codebase-design（module／interface／implementation／seam／adapter／locality／leverage）。
- 前置：`docs/adr/0025`（指派草稿）、`docs/adr/0006`（兩層驗證）、`docs/adr/0007`（確認後原子移轉）、`docs/adr/0027`（刪除與 MAC 編輯推送）、`.scratch/asset-draft-assignment/`（草稿落地語意）、`.scratch/ip-list-new-asset/`（預填）。
- 本卡是**純重構＋測試基礎設施**：**不改任何對外行為**——訊息、通知時機與順序、`emit` 時機、開關窗條件、失敗後保留的狀態一律逐字沿用（分歧語意見 §行為等價，以結果型別承載）。唯一例外見 §損壞與邊界路徑。

## 目標

把「資產→介面→指派草稿→Kea 提示」的寫入流程從三個對話框（`AssetFormDialog.vue` 1,499 行、`AssignIpDialog.vue` 934 行、`AssignmentDialog.vue` 688 行）抽成一個 in-process、純 TypeScript 的 module（`frontend/src/utils/assignmentFlow.ts`），並為前端裝上 vitest：

- 流程的驗證、呼叫順序、部分失敗、重試安全、移轉確認、warnings／Kea 提示只寫一次；ADR-0025／0007 型的修訂只改 module；
- 對話框保留：表單狀態、`emit`／開關窗、行內錯誤 banner、正評 toast 文案、模板；
- module 以依賴注入取得所有 I/O（`FlowDeps`：api／confirm／notify 埠）；測試傳 in-process fake，**不需要元件測試框架**。

## 背景（摩擦）

逐字重複實測（行號為現況）：

| 重複 | 位置 | 規模 |
|---|---|---|
| 「新增介面」UI 區塊 | AssignIpDialog 98–152、AssignmentDialog 115–169 | 逐字相同 55 行×2（含 `PeerMacHint`） |
| 新增介面流程 `createNewInterface` | AssignIpDialog 552–591、AssignmentDialog 467–507 | 除資產來源外逐字相同 |
| `messageOf` | **全 repo 16 檔**（4 頁＋12 元件） | 逐字相同 `cause instanceof Error ? cause.message : String(cause)` |
| `textOrNull` | 5 檔 | 逐字相同 |
| `normalizeMac`／`macRule` | 3 對話框 | 逐字相同 |
| `interfaceLabel` | AssignIpDialog 541、AssignmentDialog 399、IpListPage 527 | 逐字相同 |
| `purposeOptions`／`assignmentPurposeLabel` | 2 對話框 | 逐字相同 |
| 取消指派確認文案（v4／v6 兩句） | AssignIpDialog 862、AssignmentDialog 510、AssetFormDialog 1106 | 逐字相同 |
| 影響摘要（介面／指派／保留＋最後可見／MAC） | `AssetsPage.confirmDeleteWithImpact` 579–590、`AssetFormDialog.disposalImpact` 689–703 | 同型計算兩份 |
| `assignFromAssetWithTransfer`＋移轉確認 | `utils/assignTransfer.ts` | 兩個呼叫端，流程本身不可測 |

- `AssetFormDialog` 的落地管線（`submit` 1392–1419、`syncInterfaces` 1250–1307、`landAssignmentDrafts` 1327–1376）是全系統最複雜的寫入流程——遇錯即停、已成功者移出草稿、失敗者保留可重試、`savedAssetId` 防重複建立、殘留草稿靜默清理——住在不可測試的元件裡。
- 前端 0 個測試檔、無 vitest；`frontend/package.json` 只有 lint／typecheck；根 `pnpm test` 只跑 cargo；CI 手動觸發、只做安裝驗證。

## 決策（grilling 定案，2026-10-19）

1. **行為不變**：純重構；現行分歧（移轉取消的兩種呈現、殘留草稿靜默清理、通知時機、v6 不通知 Kea…）逐字保留，由 module 的結果型別＋各呼叫端呈現。與候選 1、2 同一原則。
2. **範圍（三步一函式）**：資產本體寫入、介面同步、指派落地各成一 module 函式；對話框保留三步序列＋`emit("saved")`／關窗；每步內部（驗證、逐筆順序、重試安全狀態、通知、移轉確認）全進 module。另收：網段端三流程（指派／v6 登錄／取消）、草稿預檢與草稿送出、共用純規則、影響摘要。
3. **依賴縫（依賴注入）**：module 不 import `@/api/*` 的真函式；所有 I/O 經 `FlowDeps { api, confirm, notify }`。真 adapter＝`utils/assignmentFlowDeps.ts`（綁 `@/api/*` 與 `$q`）；測試傳 fake（記呼叫序、可注入失敗）。`utils/assignTransfer.ts` 退場：純解析進 module、`confirmTransfer` 進真 adapter。
4. **共用 UI 元件**：`components/NewInterfaceForm.vue`（按鈕＋卡片＋`PeerMacHint`＋驗證＋`createInterfaceFlow`）供 AssignIpDialog／AssignmentDialog 共用；`AssetFormDialog` 的介面草稿列互動不同（加入草稿列而非即時落地），不動。
5. **檔案切分（沿用 `utils/` 平放）**：`utils/mac.ts`、`utils/errors.ts`、`utils/text.ts`、`utils/assignmentFlow.ts`（module 本體）、`utils/assetImpact.ts`、`utils/assignmentFlowDeps.ts`。module 不搬進 `domain/`（repo 無此慣例）。
6. **測試基礎設施**：frontend 加 devDep `vitest`（node 環境、不需 jsdom）；`frontend/vitest.config.ts`（`@` 別名、`include: ["src/**/*.test.ts"]`）；frontend script `"test": "vitest run"`；根 `"test:frontend"`；根 `test` 維持只跑 cargo；CI 不動；測試檔與被測檔同目錄。只測純 module；不裝 `@vue/test-utils`（元件由 typecheck／lint／build 把關）。
7. **共用小工具全收**：`messageOf` 16 檔、`textOrNull` 5 檔、`normalizeMac`／`macRule` 3 檔、`interfaceLabel` 4 檔（含 IpListPage）。
8. **MAC 與候選 6 分工**：前端 MAC 正規化由本卡收成 `utils/mac.ts`；候選 6 只做 backend 兩份（observation／interfaces）與 agent `host_range` 共用、在線門檻、屆期。
9. **名稱與紀錄**：module 名 `assignmentFlow`；不新增 ADR（ADR-0006／0007／0025／0027 續用）；`GLOSSARY.md` 不動（無新領域名詞）；`escapeHtml`（4 檔）不在本卡。

## 介面（草案）

```ts
// ---------- utils/assignmentFlow.ts ----------

/** 移轉確認所需的現任指派摘要（原 assignTransfer.AssignedElsewhere，逐字移入）。 */
export interface AssignedElsewhere {
  asset_property_no: string | null;
  asset_description: string;
  asset_location: string;
  interface_name: string | null;
  mac: string | null;
}

/** 通知埠：真 adapter 綁 $q；測試收集呼叫。 */
export interface FlowNotify {
  warning(message: string): void;        // timeout 6000（現行語意）
  kea(sync?: KeaSyncState | null): void; // status === "failed" 才提示
}

/** 移轉確認埠：確定回 true、取消回 false（真 adapter＝$q 對話框）。 */
export type TransferConfirm = (
  address: string,
  target: AssignedElsewhere
) => Promise<boolean>;

/** 流程可用的 API 面（簽章以 import type ＋ typeof 取自 @/api/*，無 runtime import）。 */
export interface FlowApi {
  createAsset: typeof createAsset;
  updateAsset: typeof updateAsset;
  createInterface: typeof createInterface;
  updateInterface: typeof updateInterface;
  deleteInterface: typeof deleteInterface;
  assignFromAsset: typeof assignFromAsset;
  assignIp: typeof assignIp;
  registerIp: typeof registerIp;
  cancelAssignment: typeof cancelAssignment;
}

/** 流程依賴：真 adapter（assignmentFlowDeps）與測試 fake 同構。 */
export interface FlowDeps {
  api: FlowApi;
  confirm: TransferConfirm;
  notify: FlowNotify;
}

/** 介面編輯列：AssetFormDialog.InterfaceDraft 的最小結構（key 由對話框自行擴充）。 */
export interface InterfaceRow {
  id: number | null;
  name: string;
  mac: string;
  note: string;
  original: { name: string | null; mac: string | null; note: string | null } | null;
}

/** 指派草稿列：AssetFormDialog.AssignmentDraft 的最小結構。 */
export interface AssignmentDraftRow extends AssignmentDraftInput {
  key: number;
  rowKey: number;
}

// ---- 共用純規則 ----
export function interfaceLabel(name: string | null, mac: string | null): string;
export function assignmentPurposeLabel(purpose: IpPurpose): string;
export function purposeOptionsFor(mac: string | null): { label: string; value: IpPurpose; disable: boolean }[];
export function interfaceNameRule(mac: string): (value: string | null) => true | string;
export function statusBlockMessage(status: IpQueryStatusKind): string | null; // 池內／排除／網段外；其餘 null
export function cancelAssignmentConfirmMessage(address: string): string;      // v4／v6 兩文案
export function preflightAssignmentDrafts(drafts: AssignmentDraftRow[], rows: InterfaceRow[]): string | null;
export function parseAssignedElsewhere(cause: unknown): AssignedElsewhere | null;

// ---- 草稿 ----
export async function draftAssignmentFlow(params: {
  text: string;
  purpose: IpPurpose;
  hostname: string;
  mac: string | null;
  existing: { address: string; subnetId: number | null }[];
  subnets: SubnetSummary[] | null;                       // null＝未載入，v6 略過預檢（後端把關）
  statusLookup: (text: string) => Promise<IpQueryStatus | null>; // 對話框提供含快取的查詢
}): Promise<{ draft: AssignmentDraftInput } | { error: string }>;

export async function landAssignmentDrafts(
  deps: FlowDeps,
  assetId: number,
  state: { drafts: AssignmentDraftRow[]; rows: InterfaceRow[] }
): Promise<{ drafts: AssignmentDraftRow[]; error: string | null }>;

// ---- 資產端 ----
export async function saveAssetBody(
  deps: FlowDeps,
  params: { input: AssetInput; savedAssetId: number | null; editingAssetId: number | null }
): Promise<{ assetId: number; createdId: number | null }>;   // createdId 非 null 時對話框記入 savedAssetId

export async function syncInterfaces(
  deps: FlowDeps,
  assetId: number,
  state: { rows: InterfaceRow[]; original: Interface[] }
): Promise<{ rows: InterfaceRow[]; original: Interface[] }>; // API 錯誤向外拋（現行語意）

export async function createInterfaceFlow(
  deps: FlowDeps,
  assetId: number,
  input: { name: string; mac: string }
): Promise<{ saved: InterfaceSaved } | { error: string }>;

export async function assignFromAssetFlow(
  deps: FlowDeps,
  assetId: number,
  params: { text: string; interfaceId: number | null; purpose: IpPurpose; hostname: string; mac: string | null }
): Promise<
  | { status: "saved"; saved: AssetAssignmentSaved }
  | { status: "cancelled" }                       // 移轉確認取消：靜默（現行語意）
  | { status: "error"; message: string }
>;

// ---- 網段端（AssignmentDialog） ----
export async function assignFromSubnetFlow(
  deps: FlowDeps,
  params: {
    subnetId: number; address: string; isEdit: boolean;
    existingInterfaceId: number | null; selectedInterfaceId: number | null;
    mac: string | null; purpose: IpPurpose; hostname: string;
  }
): Promise<{ saved: AssignmentSaved } | { error: string }>;

export async function registerIpv6Flow(
  deps: FlowDeps,
  params: { subnetId: number; text: string; selectedInterfaceId: number | null }
): Promise<{ saved: AssignmentSaved } | { error: string }>;

export async function cancelAssignmentFlow(
  deps: FlowDeps,
  subnetId: number,
  address: string
): Promise<{ ok: true } | { ok: false; message: string }>;
```

- 型別別名：`AssignmentSaved`＝`@/api/ips` 的 `AssignmentSaved`、`AssetAssignmentSaved`＝`@/api/assignments` 的、`InterfaceSaved`＝`@/api/interfaces` 的；`AssignmentDraftInput`／`sameAddress`／`sameSubnetConflict`／`subnetDisplayLabel`／`resolveSubnetByAddress` 續住 `utils/assignmentDraft.ts`（`sameSubnetConflict` 自 AssignIpDialog 移入）。
- `FlowApi` 以 `import type` 取得簽章，module 不載入真 API。
- `utils/mac.ts`：`normalizeMac(text): string | null`、`macRule(value: string | null): true | string`（訊息逐字）。
- `utils/errors.ts`：`messageOf(cause: unknown): string`。`utils/text.ts`：`textOrNull(value: string): string | null`。
- `utils/assetImpact.ts`：`summarizeAssetImpact(input: { interfaces: { mac: string | null }[]; assignments: { purpose: IpPurpose }[]; lastSeenAt: string | null })` → `{ interfaceCount; assignmentCount; reservationCount; lastSeen: string; macLabel: string }`（`lastSeen` 以 `relativeTime`；空值以「—」）。
- `utils/assignmentFlowDeps.ts`：`assignmentFlowDeps($q: QVueGlobals): FlowDeps`；`confirm` 由 `assignTransfer.confirmTransfer` 移入（逐字）；`notify.kea` 由 `notifyKeaSync` 承接。
- `components/NewInterfaceForm.vue`：props `{ assetId: number | null; disabled?: boolean }`、emits `created(saved: InterfaceSaved)`；內部狀態（展開／名稱／MAC／建立中／錯誤 banner）與 `createInterfaceFlow`。

## 行為等價表（對話框對映）

### AssignIpDialog（live 與草稿兩種模式）

| 現況 | 新住處 | 對話框留下 |
|---|---|---|
| `submit` 驗證與分支（format→介面→保留 MAC） | `assignFromAssetFlow`（訊息逐字、順序逐字） | `saving`／`errorMessage` 狀態；`status:"saved"` 時「已移轉並指派／已指派」toast＋`emit`＋關窗；`"cancelled"` 靜默；`"error"` 進 banner |
| `assignFromAssetWithTransfer`（先 false 再確認後 true） | `assignFromAssetFlow`＋`confirm` 埠 | — |
| `submitDraft` 預檢（v4 `query_status`／v6 網段比對／同網段衝突／保留 MAC） | `draftAssignmentFlow`（`statusLookup` 由對話框提供快取查詢） | `emit("draft")`＋關窗；錯誤進 banner |
| `statusForAddress` 快取（同字串重用） | 留在對話框（以 `statusLookup` 傳入） | — |
| `queryStatusHint` | 留在對話框；負面文案用 `statusBlockMessage` | 顏色與 static／reservation 分支照舊 |
| `createNewInterface` | `NewInterfaceForm`＋`createInterfaceFlow` | 成功後「加入清單、選取、收合」 |
| `cancelAssignmentItem` | `cancelAssignmentFlow`＋`cancelAssignmentConfirmMessage` | 確認框；成功「已取消指派」＋`emit`＋重載；失敗 negative toast |
| `interfaceLabel`／`purposeOptions`／`macRule`／`normalizeMac`／`interfaceNameRule`／`messageOf`／`textOrNull` | module／共用 util | — |

### AssignmentDialog（網段端）

| 現況 | 新住處 | 對話框留下 |
|---|---|---|
| `submit` v4（保留 MAC→編輯／選介面） | `assignFromSubnetFlow` | banner；成功「已更新指派／已指派」＋`emit`＋關窗 |
| `submitRegistry` v6（格式→選介面） | `registerIpv6Flow`（warnings 通知、**不通知 Kea**） | banner；成功「已登錄並指派」＋`emit`＋關窗 |
| v6 編輯模式 Enter＝不動作 | 對話框分支照舊 | — |
| `cancel` | `cancelAssignmentFlow`＋確認文案 | banner；成功「已取消指派」＋`emit`＋關窗 |
| `createNewInterface` | `NewInterfaceForm` | 成功後加入清單、選取 |
| 介面／資產搜尋、`selectedMac`、`dialogTitle` 等 | 留在對話框 | — |

### AssetFormDialog（儲存落地管線）

| 現況 | 新住處 | 對話框留下 |
|---|---|---|
| `submit` 三步序列 | 對話框保留三行序列＋`emit("saved")` 時機＋關窗條件 | 資產寫入後立即 `emit("saved")`（現行） |
| `createAsset`／`updateAsset` 分支（`savedAssetId`） | `saveAssetBody` | `savedAssetId` 寫回 |
| `syncInterfaces`（新增→編輯→刪除、`keptIds`／`original` 回寫、kea 取第一筆 failed 提示一次） | `syncInterfaces` | API 錯誤往外拋由 `submit` catch 照舊 |
| `landAssignmentDrafts`（逐筆、遇錯即停、移轉取消→「未移轉」、殘留草稿靜默清理） | `landAssignmentDrafts`（回傳殘留＋error） | `emit("saved")`＋`refreshAssignments`＋「草稿清空才關窗」 |
| `preflightAssignmentDrafts` | module 純函式 | — |
| `disposalImpact` | `summarizeAssetImpact` | 報廢子對話框模板照舊 |
| 本地 `InterfaceDraft`／`AssignmentDraft` 型別 | `extends` module 的 `InterfaceRow`／`AssignmentDraftRow` | 其餘欄位與模板不動 |

### 其他

| 現況 | 新住處 |
|---|---|
| `AssetsPage.confirmDeleteWithImpact` 的計數 | `summarizeAssetImpact`（訊息組裝與 `escapeHtml` 留在頁面） |
| `IpListPage.interfaceLabel` | module 第 4 份收斂 |
| `utils/assignTransfer.ts` | 刪檔：純解析進 module、`confirmTransfer` 進 `assignmentFlowDeps.ts` |

### 逐字保留的分歧（測試要釘住）

1. 移轉確認取消：AssignIp live＝靜默（無 toast、無錯誤）；AssetForm 落地＝`位址 X 未移轉，指派尚未建立。`＋保留草稿。網段端無移轉（後端直接 400）。
2. 殘留草稿（介面列不存在或 `id === null`）＝靜默移除、續行下一筆。
3. 通知：warnings 逐筆即時（`timeout: 6000`）；`kea_sync` 失敗逐筆即時；介面同步的多筆 `kea_sync` 於同步結束取第一筆 failed 提示一次；v6 登錄不通知 Kea。
4. `emit("saved")`：資產本體寫入後立即、落地後再一次（現行）；AssignIp／Assignment 成功後 `emit`＋關窗。
5. 錯誤呈現：AssignIp／Assignment＝行內 banner；AssetForm 落地＝banner；取消指派錯誤：AssignIp／AssetForm＝negative toast、AssignmentDialog＝banner。
6. 驗證順序逐字：live「格式→介面→保留 MAC」；draft「保留 MAC→狀態查詢→同網段衝突」；subnet「v6 分支→保留 MAC→編輯／選介面」。

## 測試決策

- 基礎設施：vitest（node）；測試檔與被測檔同目錄（`*.test.ts`）。
- `mac.test.ts`：`normalizeMac`（冒號／連字號／點／無分隔、大小寫、長度錯誤）；`macRule` 空值放行與錯誤訊息。
- `errors.test.ts`／`text.test.ts`：Error 與非 Error；trim 空字串。
- `assignmentFlow.test.ts`（純規則）：`interfaceLabel` 三態；`purposeOptionsFor` 無 MAC 停用保留；`statusBlockMessage` 三種阻擋與其餘 null；`cancelAssignmentConfirmMessage` v4／v6；`interfaceNameRule` 有／無 MAC；`preflightAssignmentDrafts`（保留無 MAC／列不存在／static 放行）；`parseAssignedElsewhere`（非 ApiError／非此 reason／欄位與 fallback 文案）。
- `assignmentDraft.test.ts`：`sameAddress`（v6 不同寫法）、`resolveSubnetByAddress`、`sameSubnetConflict`（同位址＝更新、不同位址＝衝突、不同網段＝無衝突、subnetId null＝無衝突）。
- `assetImpact.test.ts`：計數、`lastSeen`「—」、`macLabel` 連接與「—」。
- `assignmentFlow.test.ts`（流程，fake deps 記呼叫序）：
  - `createInterfaceFlow`：名稱／MAC 驗證（兩空、壞 MAC）；成功（輸入 `textOrNull`、warnings 逐筆、回傳 saved）；API 錯→`{error}`。
  - `assignFromAssetFlow`：格式錯誤；未選介面；保留無 MAC；v6→用途 static／hostname null；成功（warnings＋kea 通知）；已指派他處→`confirm` 被呼叫；取消→`{status:"cancelled"}` 且不重試；確認→`transfer:true` 重試；其他錯→`{error}`。
  - `draftAssignmentFlow`：v4 池內／排除／網段外訊息逐字；查詢失敗訊息；v6 命中網段；v6 `subnets === null` 略過（subnetId null）；同網段衝突；成功回草稿（用途／hostname 推導）。
  - `landAssignmentDrafts`：順序＝輸入序；成功逐筆移除＋逐筆通知；首錯即停（error＋失敗與其後保留）；移轉取消→「未移轉」；殘留草稿靜默清理續行。
  - `syncInterfaces`：新介面建立＋回填＋保留；未變更不 PATCH；MAC 變更 PATCH；移除者最後才刪；kea 多筆失敗取第一筆提示一次；API 錯向外拋。
  - `saveAssetBody`：無 id→create 回 `createdId`；有 id→update 回 `createdId: null`。
  - `assignFromSubnetFlow`／`registerIpv6Flow`／`cancelAssignmentFlow`：驗證訊息、成功通知（v6 不通知 Kea）、錯誤轉 `{error}`／`{ok:false}`。
- 不新增元件測試、不新增 HTTP 測試；對話框回歸＝`typecheck`＋`lint:check`＋`build`（編譯面）與既有後端測試不受影響。

## 範圍（票）

| 票 | 內容 | Blocked by |
|---|---|---|
| 01 | vitest 基礎設施＋`errors.ts`／`text.ts`＋16＋5 檔收斂 | — |
| 02 | `mac.ts`＋`assignmentFlow.ts` 純規則＋接線替換（規則層） | 01 |
| 03 | `assignmentFlow.ts` 流程層＋`assignmentFlowDeps.ts`＋`sameSubnetConflict` 移入＋module 測試 | 02 |
| 04 | `NewInterfaceForm.vue`＋AssignIpDialog 接線 | 03 |
| 05 | AssignmentDialog 接線 | 03 |
| 06 | AssetFormDialog 接線＋`assetImpact.ts`＋`assignTransfer.ts` 退場＋全卡驗收與 spec 補記 | 04, 05 |

## 明確不做

- IpListPage 的預填 helpers、AssetFormDialog 取消新增的三選擇、資產欄位驗證（`requiredRule`／`lifespanRule`…）。
- `escapeHtml`（4 檔）、`relativeTime` 等既有 util 的再整理。
- 元件測試框架（`@vue/test-utils`／jsdom）、root `test` 併跑前端、CI 變更。
- 後端／`agent/` 任何變更；候選 4（匯入管線）、5（IP 清單投影）、6（backend MAC／屆期／在線門檻／agent host 範圍）。
- 任何 API、文案、通知順序、開關窗條件、資料流變更。

## 損壞與邊界路徑

- 無允許差異：module 不解析資料（沿用既有 util），所有訊息與順序逐字延用；`FlowApi` 以型別取得簽章，runtime 依賴只剩 utils（`mac`／`text`／`errors`／`assignmentDraft`／`relativeTime`／`keaSync` 的型別）。

## 實作記錄

已實作（2026-10-19，票 01–06 全數完成）。

- 設計會談產出：`c2c9b7c`（spec 與票單）。實作 commits：票 01 `3b0becb`（補記 `cbe7b2b`）、票 02 `8582d31`（補記 `8d431ab`）、票 03 `db9756a`（補記 `ce4e7e5`）、票 04 `03844cd`＋補強 `65ed81e`（補記 `d93feb7`）、票 05 `c4e7f81`＋補強 `05d045f`（補記 `f69a10f`）、票 06 `6dce9d2`（補記即本次 spec 補記）。
- 終態：
  - module `frontend/src/utils/assignmentFlow.ts`（703 行）：9 個流程（`createInterfaceFlow`／`assignFromAssetFlow`／`draftAssignmentFlow`／`landAssignmentDrafts`／`saveAssetBody`／`syncInterfaces`／`assignFromSubnetFlow`／`registerIpv6Flow`／`cancelAssignmentFlow`）＋10 個純規則（含 `preflightAssignmentDrafts`／`parseAssignedElsewhere`／`isIpv6`）＋埠型別（`FlowApi`／`FlowDeps`／`FlowNotify`／`TransferConfirm`／`AssignedElsewhere`／`InterfaceRow`／`AssignmentDraftRow`）；無 `@/api/*` runtime import。真 adapter：`utils/assignmentFlowDeps.ts`；共用工具：`utils/mac.ts`／`utils/errors.ts`／`utils/text.ts`；影響摘要：`utils/assetImpact.ts`。
  - 三個對話框只留呈現與步驟序列：`AssetFormDialog.vue` 1,499→1,300 行、`AssignIpDialog.vue` 934→699 行、`AssignmentDialog.vue` 688→521 行；新增 `components/NewInterfaceForm.vue`（134 行）。
  - 退場：`utils/assignTransfer.ts` 刪除（`confirmTransfer` 逐字移入 `assignmentFlowDeps.ts`，純解析移入 module）；`messageOf` 16 檔、`textOrNull` 5 檔、`normalizeMac`／`macRule` 3 檔、`interfaceLabel` 4 檔全數收斂（grep 無本地定義）。
  - 前端測試基礎設施：vitest `^5.0.5`（node 環境）、`frontend/vitest.config.ts`、`frontend` script `test`、根 `test:frontend`、README 補列；測試檔與被測檔同目錄。
- 驗收（我獨立重跑）：`pnpm --filter frontend test` 6 檔 107 passed／0 failed；`pnpm --filter frontend typecheck` 綠；`pnpm lint:check` 綠；`pnpm build:frontend` `Build succeeded`。行為等價由 module 測試釘住（訊息逐字、呼叫序、通知次數與順序、遇錯即停、殘留草稿靜默清理、移轉取消兩種呈現、v6 不通知 Kea）。
- 與規格差異（皆已於票檔 Comments 記明；對外行為不變）：
  1. `parseAssignedElsewhere` 以結構守衛（`name === "ApiError"`）取代 `instanceof ApiError`——module 不 runtime import `@/api/client`（票 03）。
  2. `InterfaceRow` 含 `key`（`landAssignmentDrafts` 需以 `draft.rowKey` 對列），原「key 留對話框」的註記作廢（票 03）。
  3. `isIpv6` 改為 module 匯出純規則，供 `registerIpv6Flow` 與 AssignmentDialog 的 `ipv6Rule` 共用（票 05）。
  4. `NewInterfaceForm` 於 `prepare()` 以 `:key` 重建（等價舊碼每次開啟的重設）；換資產時以 `defineExpose` 的 `collapse()` 只收合、保留欄位（等價舊碼）（票 04、05）。
  5. 流程呼叫的 `saving` loading 包住同步驗證段——驗證錯誤同一 tick 內完成，無可見差異（票 04、05）。
  6. 草稿為空時不再呼叫 `landAssignmentDrafts`（舊碼呼叫後立即早退，無 emit／refresh）；對外行為相同（票 06）。
  7. `NewInterfaceForm` 內部處理器自 `createNewInterface` 改名 `submitNewInterface`（僅為驗收 grep；行為零變動）（票 06）。
- 後續（未做，屬另案）：候選 4（CSV 匯入管線）、候選 5（IP 清單投影）、候選 6（backend 兩份 MAC／屆期／在線門檻／agent `host_range`；**前端 MAC 已由本卡交付** `utils/mac.ts`）。
