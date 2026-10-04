import { apiDelete, apiGet, apiPost, apiPut } from "@/api/client";
import type { Warning } from "@/api/interfaces";

/** IP 狀態：可用、池內、手動設定（static）或保留（reservation）。 */
export type IpStatus = "available" | "in_pool" | "static" | "reservation";

/** 指派用途：手動設定或 DHCPv4 保留；未指派為 null。 */
export type IpPurpose = "static" | "reservation";

/** IP 清單中的指派對象（見 spec §4.3）。 */
export interface IpAssignmentTarget {
  asset_id: number;
  asset_description: string;
  asset_location: string;
  interface_id: number;
  interface_name: string | null;
  mac: string | null;
  hostname: string | null;
}

/** IP 列（見 spec §4.3；v4 由後端自網段範圍枚舉、v6 僅列登錄位址）。 */
export interface IpEntry {
  address: string;
  /** 落在 DHCP 位址池內；池內位址不可指派（v6 恆為 false）。 */
  in_pool: boolean;
  /** 是否為網段 gateway（僅標記，仍可被指派）。 */
  is_gateway: boolean;
  status: IpStatus;
  /** 指派用途；未指派為 null（v6 登錄列恆為 static）。 */
  purpose: IpPurpose | null;
  /** 指派對象（資產描述／位置、介面名稱／MAC）；未指派為 null。 */
  assignment: IpAssignmentTarget | null;
  /** 衝突標記：命中的語意規則代碼（IpInPool／IpOutOfSubnet／DuplicateHwAddress；
   *  僅標記、不阻擋，見 ADR-0006）。 */
  conflicts: string[];
}

/** 指派結果（僅記目前狀態，無歷程）。 */
export interface Assignment {
  id: number;
  subnet_id: number;
  address: string;
  interface_id: number;
  purpose: IpPurpose;
  hostname: string | null;
  created_at: string;
  updated_at: string;
}

/** 指派儲存結果：指派欄位＋不阻擋的語意警示（見 ADR-0006，沿用票 02 機制）。 */
export interface AssignmentSaved extends Assignment {
  warnings: Warning[];
}

/** 指派／改用途的輸入。 */
export interface AssignmentInput {
  interface_id: number;
  purpose: IpPurpose;
  /** 僅保留用途可填（手動設定須為 null）。 */
  hostname: string | null;
}

/** v6 登錄位址的輸入（新增即指派；用途固定 static、無 hostname）。 */
export interface RegistryInput {
  address: string;
  interface_id: number;
}

/** IP 清單搜尋與分頁參數（皆為伺服器端）。 */
export interface IpListParams {
  /** 完整位址精確比對；否則對位址文字、資產描述、位置、介面名稱與 MAC 做子字串比對。 */
  q?: string | undefined;
  /** 狀態／用途篩選。 */
  status?: IpStatus | undefined;
  page?: number | undefined;
  per_page?: number | undefined;
}

export interface IpPage {
  items: IpEntry[];
  total: number;
  page: number;
  per_page: number;
}

/** 某網段的 IP 清單（v4 全枚舉；v6 僅登錄位址，見票 06）。 */
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

/** v6 登錄位址：建立即指派（用途固定 static）；v4 網段由後端回 400。 */
export function registerIp(
  subnetId: number,
  input: RegistryInput
): Promise<AssignmentSaved> {
  return apiPost<AssignmentSaved>(`/api/v1/subnets/${subnetId}/ips`, input);
}

/** 指派或改用途（含 hostname）；結構錯誤由後端回 400 與明確訊息，
 *  語意衝突由回應 `warnings` 提示、不阻擋儲存。 */
export function assignIp(
  subnetId: number,
  address: string,
  input: AssignmentInput
): Promise<AssignmentSaved> {
  return apiPut<AssignmentSaved>(
    `/api/v1/subnets/${subnetId}/ips/${encodeURIComponent(address)}/assignment`,
    input
  );
}

/** 取消指派；位址回到「可用」。 */
export function cancelAssignment(
  subnetId: number,
  address: string
): Promise<void> {
  return apiDelete(
    `/api/v1/subnets/${subnetId}/ips/${encodeURIComponent(address)}/assignment`
  );
}
