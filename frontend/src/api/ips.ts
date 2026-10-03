import { apiGet } from "@/api/client";

/** IP 狀態：本票僅「可用」與「池內」；票 05 指派後為 static／reservation。 */
export type IpStatus = "available" | "in_pool";

/** 指派用途：手動設定或 DHCPv4 保留（票 05 實作；未指派為 null）。 */
export type IpPurpose = "static" | "reservation";

/** IP 列（見 spec §4.3；v4 位址由後端自網段範圍推導）。 */
export interface IpEntry {
  address: string;
  /** 落在 DHCP 位址池內；池內位址不可指派。 */
  in_pool: boolean;
  /** 是否為網段 gateway（僅標記，仍可被指派）。 */
  is_gateway: boolean;
  status: IpStatus;
  /** 指派用途；未指派為 null（預留欄位，票 05）。 */
  purpose: IpPurpose | null;
  /** 衝突標記；本票恆為空（預留欄位，票 07）。 */
  conflicts: string[];
}

/** IP 清單搜尋與分頁參數（皆為伺服器端）。 */
export interface IpListParams {
  /** 完整位址精確比對；否則對位址文字做子字串比對。 */
  q?: string | undefined;
  page?: number | undefined;
  per_page?: number | undefined;
}

export interface IpPage {
  items: IpEntry[];
  total: number;
  page: number;
  per_page: number;
}

/** 某網段的 IP 清單（v4 全枚舉；v6 尚未支援，見票 06）。 */
export function listSubnetIps(
  subnetId: number,
  params: IpListParams = {}
): Promise<IpPage> {
  const query = new URLSearchParams();
  for (const [key, value] of Object.entries(params)) {
    if (value !== undefined && value !== null && value !== "") {
      query.set(key, String(value));
    }
  }

  const search = query.toString();
  return apiGet<IpPage>(
    `/api/v1/subnets/${subnetId}/ips${search === "" ? "" : `?${search}`}`
  );
}
