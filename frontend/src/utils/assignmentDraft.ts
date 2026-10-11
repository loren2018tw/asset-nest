import type { IpPurpose } from "@/api/ips";
import type { SubnetSummary } from "@/api/subnets";
import {
  cidrContainsAddress,
  parseAddress,
  parseCidr,
  type ParsedAddress
} from "@/utils/cidr";

/** 指派草稿輸入：指派對話框草稿模式回傳的內容（尚未落地）。 */
export interface AssignmentDraftInput {
  /** 使用者輸入的完整位址（trim 後原文）。 */
  address: string;
  purpose: IpPurpose;
  /** 僅保留用途可填。 */
  hostname: string | null;
  /** 所屬網段 id；無法判定（如網段清單未載入的 v6）為 null。 */
  subnetId: number | null;
  /** 所屬網段顯示標籤（「名稱｜CIDR」或 CIDR）；無法判定為 null。 */
  subnetLabel: string | null;
}

/** 兩個位址是否相同（解析後數值比對；v6 不同寫法視為相同）。 */
export function sameAddress(a: string, b: string): boolean {
  const parsedA = parseAddress(a);
  const parsedB = parseAddress(b);
  if (parsedA === null || parsedB === null) {
    return a.trim() === b.trim();
  }
  return parsedA.family === parsedB.family && parsedA.value === parsedB.value;
}

/** 網段顯示標籤：「名稱｜CIDR」；無名稱或 CIDR 缺漏時僅顯示 CIDR。 */
export function subnetDisplayLabel(
  name: string | null,
  cidr: string | null
): string | null {
  if (cidr === null) {
    return null;
  }
  const text = name?.trim() ?? "";
  return text === "" ? cidr : `${text}｜${cidr}`;
}

/** 由網段清單解析位址所屬網段（v6 預檢用；網段不重疊，至多一個）。 */
export function resolveSubnetByAddress(
  subnets: SubnetSummary[],
  address: ParsedAddress
): { subnetId: number; subnetLabel: string } | null {
  for (const subnet of subnets) {
    const cidr = parseCidr(subnet.cidr);
    if (cidr === null || !cidrContainsAddress(cidr, address)) {
      continue;
    }
    return {
      subnetId: subnet.id,
      subnetLabel: subnetDisplayLabel(subnet.name, subnet.cidr) ?? subnet.cidr
    };
  }
  return null;
}
