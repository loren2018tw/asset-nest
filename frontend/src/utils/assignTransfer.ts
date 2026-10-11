import type { QVueGlobals } from "quasar";

import {
  assignFromAsset,
  type AssetAssignmentInput,
  type AssetAssignmentSaved
} from "@/api/assignments";
import { ApiError } from "@/api/client";
import { assetLabel } from "@/utils/assetLabel";

/** 後端「位址已指派給其他介面」提示所需欄位（見票 10、票 15）。 */
export interface AssignedElsewhere {
  asset_property_no: string | null;
  asset_description: string;
  asset_location: string;
  interface_name: string | null;
  mac: string | null;
}

/** 解析 `address_assigned_elsewhere` 錯誤；非此原因回傳 `null`。 */
export function parseAssignedElsewhere(
  cause: unknown
): AssignedElsewhere | null {
  if (
    !(cause instanceof ApiError) ||
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

/** 移轉確認：顯示目前指派對象，確定後回傳 true。 */
export function confirmTransfer(
  $q: QVueGlobals,
  addressText: string,
  target: AssignedElsewhere
): Promise<boolean> {
  const interfaceName = target.interface_name ?? "未命名";
  const mac = target.mac ?? "無 MAC";
  const assetText = assetLabel(
    target.asset_property_no,
    target.asset_description
  );

  return new Promise(resolve => {
    $q.dialog({
      title: "位址已指派",
      message:
        `位址 ${addressText} 目前已指派給「${assetText}（${target.asset_location}）」` +
        `的介面「${interfaceName}（${mac}）」。確定要將此指派移轉到目前介面？`,
      cancel: true,
      persistent: true
    })
      .onOk(() => resolve(true))
      .onCancel(() => resolve(false))
      .onDismiss(() => resolve(false));
  });
}

/**
 * 資產端指派（含確認後移轉，見 ADR-0007）：先以 `transfer: false` 送出；
 * 位址已指派給其他介面時顯示確認，確認後以 `transfer: true` 重試。
 * 使用者取消移轉回傳 `null`；其他錯誤拋出。
 */
export async function assignFromAssetWithTransfer(
  $q: QVueGlobals,
  assetId: number,
  input: Omit<AssetAssignmentInput, "transfer">
): Promise<AssetAssignmentSaved | null> {
  try {
    return await assignFromAsset(assetId, { ...input, transfer: false });
  } catch (cause) {
    const elsewhere = parseAssignedElsewhere(cause);
    if (elsewhere === null) {
      throw cause;
    }

    const confirmed = await confirmTransfer($q, input.address, elsewhere);
    if (!confirmed) {
      return null;
    }

    return assignFromAsset(assetId, { ...input, transfer: true });
  }
}

/** 讀取 details 中的字串欄位；缺漏或空白改用 fallback。 */
function textOf(value: unknown, fallback: string): string {
  return typeof value === "string" && value !== "" ? value : fallback;
}

function nullableTextOf(value: unknown): string | null {
  return typeof value === "string" && value !== "" ? value : null;
}
