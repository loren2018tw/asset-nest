import type { IpQueryStatusKind } from "@/api/ipCandidates";
import type { IpPurpose } from "@/api/ips";
import { parseAddress } from "@/utils/cidr";

/**
 * 指派流程共用純規則（見 `.scratch/assignment-flow/spec.md` §介面
 * 「共用純規則」）。本檔為 module 的純規則區：無 I/O、不 import 真 API；
 * 流程層（FlowDeps／submit 流程）於後續票加入。
 */

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
