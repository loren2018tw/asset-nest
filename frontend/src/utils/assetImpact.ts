import type { IpPurpose } from "@/api/ips";
import { relativeTime } from "@/utils/relativeTime";

/**
 * 資產影響摘要（見 spec §介面／§其他）：介面、指派與保留計數，加上
 * 「最後可見」與介面 MAC 的顯示字串；供資產刪除確認與報廢子對話框共用。
 */
export interface AssetImpact {
  interfaceCount: number;
  assignmentCount: number;
  reservationCount: number;
  lastSeen: string;
  macLabel: string;
}

/**
 * 計數並組出顯示字串（比照原 `AssetsPage.confirmDeleteWithImpact` 與
 * `AssetFormDialog.disposalImpact` 的兩份實作）：`lastSeenAt` 為 null 顯示
 * 「—」；MAC 取非 null 且非空者以「、」連接，全部不適用時顯示「—」。
 */
export function summarizeAssetImpact(input: {
  interfaces: { mac: string | null }[];
  assignments: { purpose: IpPurpose }[];
  lastSeenAt: string | null;
}): AssetImpact {
  const reservationCount = input.assignments.filter(
    item => item.purpose === "reservation"
  ).length;
  const macs = input.interfaces
    .map(item => item.mac)
    .filter((mac): mac is string => mac !== null && mac !== "");
  return {
    interfaceCount: input.interfaces.length,
    assignmentCount: input.assignments.length,
    reservationCount,
    lastSeen: input.lastSeenAt === null ? "—" : relativeTime(input.lastSeenAt),
    macLabel: macs.length > 0 ? macs.join("、") : "—"
  };
}
