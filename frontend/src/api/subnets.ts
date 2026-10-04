import {
  apiDelete,
  apiDownload,
  apiGet,
  apiPatch,
  apiPost,
  type DownloadedFile
} from "@/api/client";

/** 位址族：單一網段為單一地址族，雙棧以兩筆表示（見 CONTEXT.md）。 */
export type AddressFamily = "ipv4" | "ipv6";

/** DHCP 位址池（僅 IPv4；見 spec §2.3）。 */
export interface SubnetPool {
  id: number;
  start_ip: string;
  end_ip: string;
}

/** 網段（見 spec §2.3）。 */
export interface Subnet {
  id: number;
  /** 後端正規化後的 CIDR（host bits 已收斂為網路地址）。 */
  cidr: string;
  name: string | null;
  note: string | null;
  gateway: string | null;
  kea_subnet_id: number | null;
  pools: SubnetPool[];
  created_at: string;
  updated_at: string;
}

/** 列表摘要：名稱、CIDR、地址族與統計（見 spec §2.3、票 07）。 */
export interface SubnetSummary {
  id: number;
  cidr: string;
  name: string | null;
  family: AddressFamily;
  /** 已用：static＋reservation 指派數（v6 即已登錄數）。 */
  used: number;
  /** 總數：v4 為 host 數（扣 network/broadcast）；v6 為已登錄數。 */
  total: number;
  /** 衝突數：命中至少一條語意規則的指派筆數。 */
  conflicts: number;
}

/** pool 輸入（僅 IPv4）。 */
export interface SubnetPoolInput {
  start_ip: string;
  end_ip: string;
}

/** 新增／編輯內容；`null` 代表清除選填欄位；`pools` 提供時整批取代。 */
export interface SubnetInput {
  cidr: string;
  name: string | null;
  note: string | null;
  gateway: string | null;
  kea_subnet_id: number | null;
  pools: SubnetPoolInput[];
}

interface SubnetItems {
  items: SubnetSummary[];
}

export function listSubnets(): Promise<SubnetSummary[]> {
  return apiGet<SubnetItems>("/api/v1/subnets").then(result => result.items);
}

export function fetchSubnet(id: number): Promise<Subnet> {
  return apiGet<Subnet>(`/api/v1/subnets/${id}`);
}

export function createSubnet(input: SubnetInput): Promise<Subnet> {
  return apiPost<Subnet>("/api/v1/subnets", input);
}

export function updateSubnet(id: number, input: SubnetInput): Promise<Subnet> {
  return apiPatch<Subnet>(`/api/v1/subnets/${id}`, input);
}

/** 刪除網段（有指派時的防護見票 08）。 */
export function deleteSubnet(id: number): Promise<void> {
  return apiDelete(`/api/v1/subnets/${id}`);
}

/** 下載全部網段 CSV（UTF-8 BOM；格式與檔名見 ADR-0009）。 */
export function downloadSubnetsCsv(): Promise<DownloadedFile> {
  return apiDownload("/api/v1/subnets/export");
}
