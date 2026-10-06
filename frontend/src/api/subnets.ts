import {
  apiDelete,
  apiDownload,
  apiGet,
  apiPatch,
  apiPost,
  apiUpload,
  type DownloadedFile
} from "@/api/client";

/** 位址族：單一網段為單一地址族，雙棧以兩筆表示（見 GLOSSARY.md）。 */
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
  /** 觀測開關（見 ADR-0014）；v6 恆為 false。 */
  observed: boolean;
  /** 探索掃描開關（見票 05）；需 `observed`；v6 恆為 false。 */
  discovery_enabled: boolean;
  /** 探索間隔覆寫（分鐘）；`null`＝使用全站預設（見票 05）。 */
  discovery_interval_minutes: number | null;
  /** 上次探索時間（UTC）；從未探索為 `null`。 */
  last_discovery_at: string | null;
  /** 本機是否有介面位址落在該 v4 子網（同 L2；見 ADR-0015）。 */
  local: boolean;
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
  /** 觀測開關（見 ADR-0014）；v6 恆為 false。 */
  observed: boolean;
  /** 本機是否有介面位址落在該 v4 子網（同 L2）。 */
  local: boolean;
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
  /** 觀測開關；僅編輯既有 IPv4 網段有效（新增由後端預設關閉）。 */
  observed: boolean;
  /** 探索掃描開關（見票 05）；需 `observed` 且為 IPv4。 */
  discovery_enabled: boolean;
  /** 探索間隔（分鐘）；`null`＝使用全站預設（見票 05）。 */
  discovery_interval_minutes: number | null;
  pools: SubnetPoolInput[];
}

/** 匯入列問題（見 spec §5、ADR-0009）。 */
export interface SubnetImportIssue {
  severity: "warning" | "error";
  code: string;
  /** 對應的 CSV 欄位名；列級錯誤（如欄位數不一致）為 null。 */
  field: string | null;
  message: string;
}

/** 匯入列 6 欄正規化值；無法正規化者為 null。 */
export interface SubnetImportRowData {
  name: string | null;
  cidr: string | null;
  gateway: string | null;
  kea_subnet_id: number | null;
  /** 正規化位址池：每段 `起點-終點`。 */
  pools: string[];
  note: string | null;
}

/** 單列狀態：取該列最高嚴重度。 */
export type SubnetImportRowStatus = "ok" | "warning" | "error";

/** 單列報告。 */
export interface SubnetImportRow {
  /** 列號＝資料記錄序號＋1（標題為第 1 列）。 */
  row_number: number;
  status: SubnetImportRowStatus;
  data: SubnetImportRowData;
  issues: SubnetImportIssue[];
}

/** 匯入摘要。 */
export interface SubnetImportSummary {
  total: number;
  ok: number;
  warnings: number;
  errors: number;
}

/** 正式匯入的建立統計。 */
export interface SubnetImportCreated {
  subnets: number;
}

/** 匯入回應（見 spec §5）。 */
export interface SubnetImportReport {
  dry_run: boolean;
  /** 後端實際使用的編碼。 */
  encoding: "utf-8" | "big5";
  ignored_headers: string[];
  summary: SubnetImportSummary;
  rows: SubnetImportRow[];
  /** 正式匯入且寫入成功時為 true。 */
  committed: boolean;
  /** 僅 `committed=true` 時有值。 */
  created: SubnetImportCreated | null;
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

/** 掃描摘要（見 spec §HTTP API、票 02、票 05）。 */
export interface SweepReport {
  mode: "quick" | "discovery";
  /** 本次探測的目標位址數。 */
  targets: number;
  /** 有證據（ARP 回應或有效租約）的相異位址數。 */
  seen: number;
  /** 掃描耗時（毫秒）。 */
  duration_ms: number;
  /** 本輪寫入的相異網段外位址數（被動監聽；快速掃描固定 0，見票 01）。 */
  passive_seen: number;
  /** 僅探索掃描回傳：本次寫入的上次探索時間（UTC）。 */
  last_discovery_at?: string;
}

/** 手動觸發探索掃描（同步執行；前提與錯誤訊息由後端驗證，見票 05）。 */
export function discoverySweep(subnetId: number): Promise<SweepReport> {
  return apiPost<SweepReport>(`/api/v1/subnets/${subnetId}/sweeps`, {
    mode: "discovery"
  });
}

/** 下載全部網段 CSV（UTF-8 BOM；格式與檔名見 ADR-0009）。 */
export function downloadSubnetsCsv(): Promise<DownloadedFile> {
  return apiDownload("/api/v1/subnets/export");
}

/**
 * 匯入網段 CSV（multipart，檔案欄位 `file`；見 spec §5）。
 * `dryRun=true` 只做預覽與驗證，不寫入任何資料。
 */
export function importSubnets(
  file: File,
  dryRun: boolean
): Promise<SubnetImportReport> {
  return apiUpload<SubnetImportReport>(
    `/api/v1/subnets/import?dry_run=${dryRun}`,
    file
  );
}
