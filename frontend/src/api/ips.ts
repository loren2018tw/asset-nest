import { apiDelete, apiGet, apiPost, apiPut } from "@/api/client";
import type { Warning } from "@/api/interfaces";
import type { KeaSyncState } from "@/api/kea";

/** IP 狀態：可用、池內、手動設定（static）或保留（reservation）。 */
export type IpStatus = "available" | "in_pool" | "static" | "reservation";

/** 指派用途：手動設定或 DHCPv4 保留；未指派為 null。 */
export type IpPurpose = "static" | "reservation";

/** IP 清單中的指派對象（見 spec §4.3）。 */
export interface IpAssignmentTarget {
  asset_id: number;
  /** 資產財產編號；未填為 null（對話框顯示「財產編號(描述)」，見票 15）。 */
  asset_property_no: string | null;
  asset_description: string;
  /** 資產廠牌／型號；未填為 null（IP 清單第一行顯示「描述(廠牌 型號)」，見票 16）。 */
  asset_brand: string | null;
  asset_model: string | null;
  asset_location: string;
  interface_id: number;
  interface_name: string | null;
  mac: string | null;
  hostname: string | null;
}

/** 觀測來源：本地 ARP 探測或 Kea 租約（見 GLOSSARY.md「觀測詞彙」）。 */
export type IpSeenSource = "arp" | "kea_lease";

/** IP 列（見 spec §4.3；v4 由後端自網段範圍枚舉、v6 僅列登錄位址）。 */
export interface IpEntry {
  address: string;
  /** 落在 DHCP 位址池內；池內位址不可指派（v6 恆為 false）。 */
  in_pool: boolean;
  status: IpStatus;
  /** 指派用途；未指派為 null（v6 登錄列恆為 static）。 */
  purpose: IpPurpose | null;
  /** 指派對象（資產描述／位置、介面名稱／MAC）；未指派為 null。 */
  assignment: IpAssignmentTarget | null;
  /** 衝突標記：命中的語意規則代碼（IpInPool／IpOutOfSubnet／DuplicateHwAddress；
   *  僅標記、不阻擋，見 ADR-0006）。 */
  conflicts: string[];
  /** 最後可見時間（UTC）；從未上線為 null（見票 02）。 */
  last_seen_at: string | null;
  /** 最後可見 MAC（小寫冒號格式）；從未上線為 null。 */
  last_seen_mac: string | null;
  /** 最後可見來源；從未上線為 null。 */
  last_seen_source: IpSeenSource | null;
  /** 最後檢查時間；尚未掃描為 null。 */
  last_checked_at: string | null;
  /** 有效觀測涵蓋（網段已開觀測且本機同 L2；v6 恆為 false）；據此區分
   *  「未觀測」與「從未上線」。 */
  observed: boolean;
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

/** 指派儲存結果：指派欄位＋不阻擋的語意警示（見 ADR-0006）＋Kea 推送結果
 *  （僅在應同步時出現，見 ADR-0011）。 */
export interface AssignmentSaved extends Assignment {
  warnings: Warning[];
  kea_sync?: KeaSyncState;
}

/** 取消指派結果：`kea_sync` 僅在應同步時出現（見 ADR-0011）。 */
export interface AssignmentDeleted {
  kea_sync?: KeaSyncState;
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

/** IP 清單可排序欄位（後端白名單；見 spec §5、票 14；`last_seen` 為票 02）。 */
export type IpSortField =
  | "address"
  | "status"
  | "location"
  | "assignment"
  | "last_seen";

/** IP 清單搜尋、排序與分頁參數（皆為伺服器端）。 */
export interface IpListParams {
  /** 完整位址精確比對；否則對位址文字、資產描述、位置、介面名稱與 MAC 做子字串比對。 */
  q?: string | undefined;
  /** 狀態／用途篩選。 */
  status?: IpStatus | undefined;
  /** 排序欄位；預設 address。 */
  sort?: IpSortField | undefined;
  /** 排序方向；預設 asc。 */
  dir?: "asc" | "desc" | undefined;
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

/** 取消指派；位址回到「可用」。回應含 Kea 推送結果（如適用，見 ADR-0011）。 */
export function cancelAssignment(
  subnetId: number,
  address: string
): Promise<AssignmentDeleted> {
  return apiDelete<AssignmentDeleted>(
    `/api/v1/subnets/${subnetId}/ips/${encodeURIComponent(address)}/assignment`
  );
}

/** 掃描摘要（見 spec §HTTP API）。 */
export interface SweepReport {
  mode: string;
  /** 本次探測的目標位址數（快速掃描＝已指派位址）。 */
  targets: number;
  /** 有回應的目標位址數。 */
  seen: number;
  /** 掃描耗時（毫秒）。 */
  duration_ms: number;
}

/** 手動觸發快速掃描（同步執行；前提與錯誤訊息由後端驗證，見票 02）。 */
export function quickSweep(subnetId: number): Promise<SweepReport> {
  return apiPost<SweepReport>(`/api/v1/subnets/${subnetId}/sweeps`, {
    mode: "quick"
  });
}
