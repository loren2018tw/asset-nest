import type { AssetInput, createAsset, updateAsset } from "@/api/assets";
import type { AssetAssignmentSaved, assignFromAsset } from "@/api/assignments";
import type { ApiError } from "@/api/client";
import type {
  Interface,
  InterfaceInput,
  InterfaceSaved,
  createInterface,
  deleteInterface,
  updateInterface
} from "@/api/interfaces";
import type { IpQueryStatus, IpQueryStatusKind } from "@/api/ipCandidates";
import type {
  AssignmentSaved,
  IpPurpose,
  assignIp,
  cancelAssignment,
  registerIp
} from "@/api/ips";
import type { KeaSyncState } from "@/api/kea";
import type { SubnetSummary } from "@/api/subnets";
import {
  resolveSubnetByAddress,
  sameSubnetConflict,
  subnetDisplayLabel,
  type AssignmentDraftInput
} from "@/utils/assignmentDraft";
import { parseAddress } from "@/utils/cidr";
import { messageOf } from "@/utils/errors";
import { normalizeMac } from "@/utils/mac";
import { textOrNull } from "@/utils/text";

/**
 * 指派流程 module（見 `.scratch/assignment-flow/spec.md` §介面）：共用純規則、
 * 流程函式與 I/O 埠。不 runtime import `@/api/*`——API 簽章以 `import type`
 * 取得，所有 I/O 經 `FlowDeps`（真 adapter＝`utils/assignmentFlowDeps.ts`；
 * 測試傳 in-process fake）。行為逐字沿用三個對話框的現況：訊息、呼叫順序、
 * 通知時機與次數、遇錯即停、殘留草稿靜默清理、移轉取消語意。
 */

// ---------- 型別：流程埠與列結構 ----------

/** 移轉確認所需的現任指派摘要（原 assignTransfer.AssignedElsewhere，逐字移入）。 */
export interface AssignedElsewhere {
  asset_property_no: string | null;
  asset_description: string;
  asset_location: string;
  interface_name: string | null;
  mac: string | null;
}

/** 通知埠：真 adapter 綁 `$q`；測試收集呼叫。 */
export interface FlowNotify {
  /** 語意警示（現行 `timeout: 6000`）。 */
  warning(message: string): void;
  /** Kea 推送結果；`status === "failed"` 才提示。 */
  kea(sync?: KeaSyncState | null): void;
}

/** 移轉確認埠：確定回 true、取消回 false（真 adapter＝`$q` 對話框）。 */
export type TransferConfirm = (
  address: string,
  target: AssignedElsewhere
) => Promise<boolean>;

/** 流程可用的 API 面；簽章以 `import type` ＋ `typeof` 取自 `@/api/*`，無 runtime import。 */
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

/**
 * 介面編輯列：AssetFormDialog.InterfaceDraft 的最小結構。`key` 為列身分
 * （指派草稿以 `rowKey` 對應）；對話框其餘欄位自行擴充。
 */
export interface InterfaceRow {
  key: number;
  id: number | null;
  name: string;
  mac: string;
  note: string;
  original: {
    name: string | null;
    mac: string | null;
    note: string | null;
  } | null;
}

/** 指派草稿列：AssetFormDialog.AssignmentDraft 的最小結構。 */
export interface AssignmentDraftRow extends AssignmentDraftInput {
  key: number;
  rowKey: number;
}

// ---------- 共用純規則 ----------

/** 介面顯示：名稱／MAC；無 MAC 時標示（三個對話框與 IP 清單共用）。 */
export function interfaceLabel(
  name: string | null,
  mac: string | null
): string {
  const label = name ?? "未命名";
  return mac === null ? `${label}（無 MAC）` : `${label} ｜ ${mac}`;
}

/** 指派用途標籤（清單顯示）。 */
export function assignmentPurposeLabel(purpose: IpPurpose): string {
  return purpose === "reservation" ? "保留" : "手動設定";
}

/** 用途選項：無 MAC 時保留（reservation）停用（見 spec §3）。 */
export function purposeOptionsFor(
  mac: string | null
): { label: string; value: IpPurpose; disable: boolean }[] {
  return [
    { label: "手動設定（static）", value: "static", disable: false },
    {
      label: "DHCPv4 保留（reservation）",
      value: "reservation",
      disable: mac === null
    }
  ];
}

/** MAC 空白時名稱必填（見 spec §3.1）；有 MAC 時名稱選填。 */
export function interfaceNameRule(
  mac: string
): (value: string | null) => true | string {
  return value => {
    if ((value ?? "").trim() !== "") {
      return true;
    }
    return mac.trim() !== "" || "MAC 空白時名稱為必填";
  };
}

/**
 * 基本 IPv6 格式驗證（支援 :: 壓縮與 IPv4-mapped）；供網段端 UI 規則
 * （`AssignmentDialog.ipv6Rule`）與 `registerIpv6Flow` 共用；網段歸屬仍由
 * 後端驗證。
 */
export function isIpv6(value: string): boolean {
  const text = value.trim();
  if (!text.includes(":")) {
    return false;
  }
  try {
    return new URL(`http://[${text}]/`).hostname.startsWith("[");
  } catch {
    return false;
  }
}

/** 位址狀態的阻擋訊息（見 spec §10.3）：池內／排除／網段外；其餘回 `null`。 */
export function statusBlockMessage(status: IpQueryStatusKind): string | null {
  switch (status) {
    case "in_pool":
      return "此位址在 DHCP 位址池內，不可指派";
    case "excluded":
      return "此位址在排除範圍內，不可指派";
    case "out_of_subnet":
      return "此位址不在任何網段可指派的範圍內";
    default:
      return null;
  }
}

/** 取消指派確認文案；v6 取消後自登錄清單移除（見票 06）。 */
export function cancelAssignmentConfirmMessage(address: string): string {
  return parseAddress(address)?.family === "ipv6"
    ? `確定要取消 ${address} 的指派？取消後該位址將自登錄清單移除。`
    : `確定要取消 ${address} 的指派？取消後該位址回到「可用」。`;
}

/** 解析 `address_assigned_elsewhere` 錯誤；非此原因回傳 `null`。 */
export function parseAssignedElsewhere(
  cause: unknown
): AssignedElsewhere | null {
  if (
    !isApiError(cause) ||
    cause.details?.reason !== "address_assigned_elsewhere"
  ) {
    return null;
  }

  const details = cause.details;
  return {
    asset_property_no: nullableTextOf(details.asset_property_no),
    asset_description: textOf(details.asset_description, "未知資產"),
    asset_location: textOf(details.asset_location, "未知位置"),
    interface_name: nullableTextOf(details.interface_name),
    mac: nullableTextOf(details.mac)
  };
}

/** 儲存前預檢指派草稿：保留需介面 MAC（見 spec §5.1）；無誤回 `null`。 */
export function preflightAssignmentDrafts(
  drafts: AssignmentDraftRow[],
  rows: InterfaceRow[]
): string | null {
  for (const draft of drafts) {
    if (draft.purpose !== "reservation") {
      continue;
    }
    const row = rows.find(item => item.key === draft.rowKey);
    if (row === undefined || normalizeMac(row.mac) === null) {
      return `保留指派 ${draft.address} 需要介面 MAC；請補上 MAC 或改為手動設定。`;
    }
  }
  return null;
}

// ---------- 流程 ----------

/** 當場新增介面（名稱／MAC）；成功後 warnings 逐筆即時提示。 */
export async function createInterfaceFlow(
  deps: FlowDeps,
  assetId: number,
  input: { name: string; mac: string }
): Promise<{ saved: InterfaceSaved } | { error: string }> {
  const name = input.name.trim();
  const mac = input.mac.trim();

  if (name === "" && mac === "") {
    return { error: "MAC 空白時名稱為必填" };
  }
  if (mac !== "" && normalizeMac(mac) === null) {
    return {
      error: "MAC 格式錯誤：須為 12 位十六進位（可用冒號、連字號或無分隔）"
    };
  }

  try {
    const saved = await deps.api.createInterface(assetId, {
      name: textOrNull(name),
      mac: textOrNull(mac),
      note: null
    });
    for (const warning of saved.warnings) {
      deps.notify.warning(warning.message);
    }
    return { saved };
  } catch (cause) {
    return { error: messageOf(cause) };
  }
}

/**
 * 資產端指派（含確認後移轉，見 ADR-0007）：驗證「格式→介面→保留 MAC」；
 * 先以 `transfer: false` 送出，位址已指派給其他介面時經 `confirm` 確認後
 * 以 `transfer: true` 重試。確認被取消回 `cancelled`（靜默，現行語意）。
 */
export async function assignFromAssetFlow(
  deps: FlowDeps,
  assetId: number,
  params: {
    text: string;
    interfaceId: number | null;
    purpose: IpPurpose;
    hostname: string;
    mac: string | null;
  }
): Promise<
  | { status: "saved"; saved: AssetAssignmentSaved }
  | { status: "cancelled" }
  | { status: "error"; message: string }
> {
  const parsed = parseAddress(params.text);
  if (parsed === null) {
    return {
      status: "error",
      message: "IP 位址格式錯誤（例：10.0.0.5、fd00::5）"
    };
  }
  if (params.interfaceId === null) {
    return { status: "error", message: "請選擇介面" };
  }
  if (params.purpose === "reservation" && params.mac === null) {
    return { status: "error", message: "所選介面無 MAC，不可設為保留" };
  }

  const isV6 = parsed.family === "ipv6";
  const input = {
    address: params.text,
    interface_id: params.interfaceId,
    purpose: isV6 ? ("static" as const) : params.purpose,
    hostname:
      !isV6 && params.purpose === "reservation"
        ? textOrNull(params.hostname)
        : null
  };

  try {
    let saved: AssetAssignmentSaved;
    try {
      saved = await deps.api.assignFromAsset(assetId, {
        ...input,
        transfer: false
      });
    } catch (cause) {
      const elsewhere = parseAssignedElsewhere(cause);
      if (elsewhere === null) {
        return { status: "error", message: messageOf(cause) };
      }
      const confirmed = await deps.confirm(params.text, elsewhere);
      if (!confirmed) {
        return { status: "cancelled" };
      }
      saved = await deps.api.assignFromAsset(assetId, {
        ...input,
        transfer: true
      });
    }

    for (const warning of saved.warnings) {
      deps.notify.warning(warning.message);
    }
    deps.notify.kea(saved.kea_sync);
    return { status: "saved", saved };
  } catch (cause) {
    return { status: "error", message: messageOf(cause) };
  }
}

/**
 * 草稿模式送出（見 spec §4.3）：預檢後回傳指派草稿（不落地）。
 * v4 以 `statusLookup` 擋池內／排除／不在網段；v6 以網段清單比對；
 * 同介面同網段同位址＝更新、不同位址＝擋下。
 */
export async function draftAssignmentFlow(params: {
  text: string;
  purpose: IpPurpose;
  hostname: string;
  mac: string | null;
  existing: { address: string; subnetId: number | null }[];
  /** null＝未載入，v6 略過預檢（後端把關）。 */
  subnets: SubnetSummary[] | null;
  statusLookup: (text: string) => Promise<IpQueryStatus | null>;
}): Promise<{ draft: AssignmentDraftInput } | { error: string }> {
  const parsed = parseAddress(params.text);
  if (parsed === null) {
    return { error: "IP 位址格式錯誤（例：10.0.0.5、fd00::5）" };
  }

  if (params.purpose === "reservation" && params.mac === null) {
    return { error: "所選介面無 MAC，不可設為保留" };
  }

  const isV6 = parsed.family === "ipv6";
  const purposeValue: IpPurpose = isV6 ? "static" : params.purpose;
  const hostnameValue =
    !isV6 && purposeValue === "reservation"
      ? textOrNull(params.hostname)
      : null;

  let subnetId: number | null = null;
  let subnetLabel: string | null = null;

  if (!isV6) {
    const status = await params.statusLookup(params.text);
    if (status === null) {
      return {
        error: `無法查詢位址 ${params.text} 的狀態；請確認格式正確，或稍後再試`
      };
    }
    const block = statusBlockMessage(status.status);
    if (block !== null) {
      return { error: block };
    }
    subnetId = status.subnet_id;
    subnetLabel = subnetDisplayLabel(status.subnet_name, status.subnet_cidr);
  } else if (params.subnets !== null) {
    const resolved = resolveSubnetByAddress(params.subnets, parsed);
    if (resolved === null) {
      return { error: `位址 ${params.text} 不在任何網段內` };
    }
    subnetId = resolved.subnetId;
    subnetLabel = resolved.subnetLabel;
  }

  const conflict = sameSubnetConflict(params.existing, params.text, subnetId);
  if (conflict !== null) {
    return {
      error: `此介面在此網段已有位址 ${conflict}（同一介面同一網段至多一個位址）`
    };
  }

  return {
    draft: {
      address: params.text,
      purpose: purposeValue,
      hostname: hostnameValue,
      subnetId,
      subnetLabel
    }
  };
}

/**
 * 逐筆落地指派草稿（見 spec §5.2）：依輸入序、遇錯即停；已成功者自清單
 * 移除並逐筆提示，失敗者保留供重試。移轉確認被取消視同失敗並回「未移轉」
 * 訊息（現行語意）；介面列不存在或未落地（`id === null`）＝殘留草稿，
 * 靜默移除、續行。
 */
export async function landAssignmentDrafts(
  deps: FlowDeps,
  assetId: number,
  state: { drafts: AssignmentDraftRow[]; rows: InterfaceRow[] }
): Promise<{ drafts: AssignmentDraftRow[]; error: string | null }> {
  let remaining = state.drafts;

  for (const draft of state.drafts) {
    const row = state.rows.find(item => item.key === draft.rowKey);
    if (row === undefined || row.id === null) {
      // 介面已不存在（理論上草稿已連動移除）；僅清理殘留
      remaining = remaining.filter(item => item.key !== draft.key);
      continue;
    }

    const input = {
      address: draft.address,
      interface_id: row.id,
      purpose: draft.purpose,
      hostname: draft.hostname
    };

    let saved: AssetAssignmentSaved;
    try {
      saved = await deps.api.assignFromAsset(assetId, {
        ...input,
        transfer: false
      });
    } catch (cause) {
      const elsewhere = parseAssignedElsewhere(cause);
      if (elsewhere === null) {
        return { drafts: remaining, error: messageOf(cause) };
      }
      const confirmed = await deps.confirm(draft.address, elsewhere);
      if (!confirmed) {
        return {
          drafts: remaining,
          error: `位址 ${draft.address} 未移轉，指派尚未建立。`
        };
      }
      try {
        saved = await deps.api.assignFromAsset(assetId, {
          ...input,
          transfer: true
        });
      } catch (retryCause) {
        return { drafts: remaining, error: messageOf(retryCause) };
      }
    }

    for (const warning of saved.warnings) {
      deps.notify.warning(warning.message);
    }
    deps.notify.kea(saved.kea_sync);
    remaining = remaining.filter(item => item.key !== draft.key);
  }

  return { drafts: remaining, error: null };
}

/**
 * 資產本體寫入：`savedAssetId` 優先（重試時不重複建立）；新建回報
 * `createdId` 供對話框記入，更新為 `null`。API 錯誤向外拋（現行語意）。
 */
export async function saveAssetBody(
  deps: FlowDeps,
  params: {
    input: AssetInput;
    savedAssetId: number | null;
    editingAssetId: number | null;
  }
): Promise<{ assetId: number; createdId: number | null }> {
  const assetId = params.savedAssetId ?? params.editingAssetId;
  if (assetId === null) {
    const created = await deps.api.createAsset(params.input);
    return { assetId: created.id, createdId: created.id };
  }

  await deps.api.updateAsset(assetId, params.input);
  return { assetId, createdId: null };
}

/**
 * 儲存介面：新增／編輯後再刪除已移除者；`keptIds` 保留已落地列、回寫
 * `original`。多筆 Kea 推送失敗僅取第一筆提示一次、於同步結束後提示，
 * 其後逐筆提示 warnings（現況順序）。API 錯誤向外拋（現行語意）。
 */
export async function syncInterfaces(
  deps: FlowDeps,
  assetId: number,
  state: { rows: InterfaceRow[]; original: Interface[] }
): Promise<{ rows: InterfaceRow[]; original: Interface[] }> {
  const rows = state.rows;
  let original = state.original;
  const warnings: string[] = [];
  const keaSyncs: KeaSyncState[] = [];
  const keptIds = new Set<number>();

  for (const row of rows) {
    const input = toInterfaceInput(row);

    if (row.id === null) {
      const saved = await deps.api.createInterface(assetId, input);
      row.id = saved.id;
      row.name = saved.name ?? "";
      row.mac = saved.mac ?? "";
      row.note = saved.note ?? "";
      row.original = { name: saved.name, mac: saved.mac, note: saved.note };
      // 新介面已落地：納入既有介面並保留（供後續指派／重試；避免下次儲存誤刪）
      keptIds.add(saved.id);
      original.push(saved);
      warnings.push(...saved.warnings.map(warning => warning.message));
      continue;
    }

    keptIds.add(row.id);
    if (!interfaceChanged(row)) {
      continue;
    }

    const saved = await deps.api.updateInterface(row.id, input);
    row.name = saved.name ?? "";
    row.mac = saved.mac ?? "";
    row.note = saved.note ?? "";
    row.original = { name: saved.name, mac: saved.mac, note: saved.note };
    warnings.push(...saved.warnings.map(warning => warning.message));
    keaSyncs.push(...(saved.kea_sync ?? []));
  }

  // 已移除的既有介面：在新增／編輯成功後才刪除
  const removed: number[] = [];
  for (const item of original) {
    if (keptIds.has(item.id)) {
      continue;
    }
    const deleted = await deps.api.deleteInterface(item.id);
    keaSyncs.push(...deleted.kea_sync);
    removed.push(item.id);
  }
  original = original.filter(item => !removed.includes(item.id));

  // 多筆失敗僅提示一次（訊息比照 utils/keaSync.ts）
  deps.notify.kea(keaSyncs.find(sync => sync.status === "failed"));
  for (const message of warnings) {
    deps.notify.warning(message);
  }

  return { rows, original };
}

/**
 * 網段端 v4 指派（編輯或選介面）：驗證「保留 MAC→編輯／選介面」；
 * 成功回傳 `saved`（warnings 逐筆即時提示、Kea 提示照舊）。
 */
export async function assignFromSubnetFlow(
  deps: FlowDeps,
  params: {
    subnetId: number;
    address: string;
    isEdit: boolean;
    existingInterfaceId: number | null;
    selectedInterfaceId: number | null;
    mac: string | null;
    purpose: IpPurpose;
    hostname: string;
  }
): Promise<{ saved: AssignmentSaved } | { error: string }> {
  if (params.purpose === "reservation" && params.mac === null) {
    return { error: "所選介面無 MAC，不可設為保留" };
  }

  let interfaceId: number;
  if (params.isEdit) {
    if (params.existingInterfaceId === null) {
      return { error: "缺少指派資料" };
    }
    interfaceId = params.existingInterfaceId;
  } else {
    if (params.selectedInterfaceId === null) {
      return { error: "請選擇介面" };
    }
    interfaceId = params.selectedInterfaceId;
  }

  try {
    const saved = await deps.api.assignIp(params.subnetId, params.address, {
      interface_id: interfaceId,
      purpose: params.purpose,
      hostname:
        params.purpose === "reservation" ? textOrNull(params.hostname) : null
    });
    for (const warning of saved.warnings) {
      deps.notify.warning(warning.message);
    }
    deps.notify.kea(saved.kea_sync);
    return { saved };
  } catch (cause) {
    return { error: messageOf(cause) };
  }
}

/**
 * 網段端 v6 登錄（輸入位址＋選介面，建立即指派；用途固定 static）。
 * warnings 逐筆提示、**不通知 Kea**（v6 不經 Kea，現行語意）。
 */
export async function registerIpv6Flow(
  deps: FlowDeps,
  params: {
    subnetId: number;
    text: string;
    selectedInterfaceId: number | null;
  }
): Promise<{ saved: AssignmentSaved } | { error: string }> {
  if (!isIpv6(params.text)) {
    return { error: "IPv6 位址格式錯誤（例：fd00::10）" };
  }
  if (params.selectedInterfaceId === null) {
    return { error: "請選擇介面" };
  }

  try {
    const saved = await deps.api.registerIp(params.subnetId, {
      address: params.text,
      interface_id: params.selectedInterfaceId
    });
    for (const warning of saved.warnings) {
      deps.notify.warning(warning.message);
    }
    return { saved };
  } catch (cause) {
    return { error: messageOf(cause) };
  }
}

/** 取消指派；成功先提示 Kea 推送結果再回 `ok`。 */
export async function cancelAssignmentFlow(
  deps: FlowDeps,
  subnetId: number,
  address: string
): Promise<{ ok: true } | { ok: false; message: string }> {
  try {
    const result = await deps.api.cancelAssignment(subnetId, address);
    deps.notify.kea(result.kea_sync);
    return { ok: true };
  } catch (cause) {
    return { ok: false, message: messageOf(cause) };
  }
}

/** `ApiError` 的結構守衛：真 `ApiError` 建構時固定 `name = "ApiError"`。
 *  module 不 runtime import `@/api/client`（見 spec §損壞與邊界路徑），
 *  故以名稱代替 `instanceof`；測試傳入的真 `ApiError` 一律通過。 */
function isApiError(cause: unknown): cause is ApiError {
  return cause instanceof Error && cause.name === "ApiError";
}

/** 與上次儲存值比較（MAC 以正規化後比對），避免無謂的 PATCH。 */
function interfaceChanged(row: InterfaceRow): boolean {
  const original = row.original;
  if (original === null) {
    return true;
  }
  return (
    row.name.trim() !== (original.name ?? "") ||
    (normalizeMac(row.mac) ?? "") !== (original.mac ?? "") ||
    row.note.trim() !== (original.note ?? "")
  );
}

function toInterfaceInput(row: InterfaceRow): InterfaceInput {
  return {
    name: textOrNull(row.name),
    mac: textOrNull(row.mac),
    note: textOrNull(row.note)
  };
}

/** 讀取 details 中的字串欄位；缺漏或空白改用 fallback。 */
function textOf(value: unknown, fallback: string): string {
  return typeof value === "string" && value !== "" ? value : fallback;
}

function nullableTextOf(value: unknown): string | null {
  return typeof value === "string" && value !== "" ? value : null;
}
