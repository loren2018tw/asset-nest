import {
  apiDelete,
  apiDownload,
  apiGet,
  apiPatch,
  apiPost,
  apiUpload,
  type DownloadedFile
} from "@/api/client";
import type { Interface } from "@/api/interfaces";
import type { IpPurpose } from "@/api/ips";

/** 資產（見 spec §2.1；`expired` 由後端計算）。 */
export interface Asset {
  id: number;
  property_no: string | null;
  description: string;
  location: string;
  device_serial: string | null;
  brand: string | null;
  model: string | null;
  purchase_date: string | null;
  lifespan_years: number | null;
  note: string | null;
  /** 標籤：多值自由文字（後端正規化、不分大小寫去重）。 */
  tags: string[];
  /** 屆齡：購置日期＋年限早於今天（僅提示）。 */
  expired: boolean;
  created_at: string;
  updated_at: string;
}

/** 資產清單列：資產欄位＋全部已指派 IP（見 spec §2.1、票 12）。 */
export interface AssetListRow extends Asset {
  /** 全部已指派位址（跨介面、跨網段；v4 先、v6 後，同地址族依數值）。 */
  assigned_ips: string[];
}

/** 新增／編輯表單內容；`null` 或空字串代表清除選填欄位。 */
export interface AssetInput {
  property_no: string | null;
  description: string;
  location: string;
  device_serial: string | null;
  brand: string | null;
  model: string | null;
  purchase_date: string | null;
  lifespan_years: number | null;
  note: string | null;
  tags: string[];
}

/** 清單搜尋、篩選、排序與分頁參數（皆為伺服器端）。 */
export interface AssetListParams {
  q?: string | undefined;
  location?: string | undefined;
  brand?: string | undefined;
  device_serial?: string | undefined;
  /** 標籤：不分大小寫完全符合。 */
  tag?: string | undefined;
  /** 排序欄位（後端白名單；無效值回 400）。 */
  sort?: string | undefined;
  dir?: "asc" | "desc" | undefined;
  page?: number | undefined;
  per_page?: number | undefined;
}

/** 匯出參數：與清單相同，但忽略分頁（後端亦忽略 `page`／`per_page`；見 spec §4）。 */
export type AssetExportParams = Omit<AssetListParams, "page" | "per_page">;

export interface AssetPage {
  /** 清單列含已指派 IP；POST／PATCH 回應的 `Asset` 不含此欄位。 */
  items: AssetListRow[];
  total: number;
  page: number;
  per_page: number;
}

/** 資產詳情中的已指派 IP（唯讀顯示；含網段資訊，見 spec §4.1、§5）。 */
export interface AssetAssignment {
  id: number;
  subnet_id: number;
  subnet_cidr: string;
  subnet_name: string | null;
  address: string;
  purpose: IpPurpose;
  hostname: string | null;
  interface_id: number;
  interface_name: string | null;
  mac: string | null;
  created_at: string;
  updated_at: string;
}

/** 資產詳情：資產欄位＋介面清單＋已指派 IP（見 spec §5）。 */
export interface AssetDetail extends Asset {
  interfaces: Interface[];
  assignments: AssetAssignment[];
}

/** 匯入列問題（見 spec §5.2、ADR-0008）。 */
export interface ImportIssue {
  severity: "warning" | "error";
  code: string;
  /** 對應的 CSV 欄位名；列級錯誤（如欄位數不一致）為 null。 */
  field: string | null;
  message: string;
}

/** 匯入列 14 欄正規化值；無法正規化者為 null。 */
export interface ImportRowData {
  property_no: string | null;
  description: string | null;
  location: string | null;
  device_serial: string | null;
  brand: string | null;
  model: string | null;
  purchase_date: string | null;
  lifespan_years: number | null;
  note: string | null;
  tags: string[];
  mac: string | null;
  ipv4: string | null;
  ipv6: string | null;
  hostname: string | null;
}

/** 單列狀態：取該列最高嚴重度（見 spec §5.2）。 */
export type ImportRowStatus = "ok" | "warning" | "error";

/** 單列報告。 */
export interface ImportRow {
  /** 列號＝資料記錄序號＋1（標題為第 1 列）。 */
  row_number: number;
  status: ImportRowStatus;
  data: ImportRowData;
  issues: ImportIssue[];
}

/** 匯入摘要。 */
export interface ImportSummary {
  total: number;
  ok: number;
  warnings: number;
  errors: number;
}

/** 正式匯入的建立統計。 */
export interface ImportCreated {
  assets: number;
  interfaces: number;
  assignments: number;
}

/** 匯入回應（見 spec §5.2）。 */
export interface ImportReport {
  dry_run: boolean;
  /** 後端實際使用的編碼。 */
  encoding: "utf-8" | "big5";
  ignored_headers: string[];
  summary: ImportSummary;
  rows: ImportRow[];
  /** 正式匯入且寫入成功時為 true。 */
  committed: boolean;
  /** 僅 `committed=true` 時有值。 */
  created: ImportCreated | null;
}

/**
 * 匯入資產 CSV（multipart，檔案欄位 `file`；見 spec §5.1）。
 * `dryRun=true` 只做預覽與驗證，不寫入任何資料。
 */
export function importAssets(
  file: File,
  dryRun: boolean
): Promise<ImportReport> {
  return apiUpload<ImportReport>(
    `/api/v1/assets/import?dry_run=${dryRun}`,
    file
  );
}

interface StringItems {
  items: string[];
}

/** 將參數轉為查詢字串（含 `?`）；略過 undefined／null／空字串。 */
function queryString(params: object): string {
  const query = new URLSearchParams();
  for (const [key, value] of Object.entries(params)) {
    if (value !== undefined && value !== null && value !== "") {
      query.set(key, String(value));
    }
  }

  const search = query.toString();
  return search === "" ? "" : `?${search}`;
}

export function listAssets(params: AssetListParams = {}): Promise<AssetPage> {
  return apiGet<AssetPage>(`/api/v1/assets${queryString(params)}`);
}

/**
 * 下載目前搜尋／篩選／排序的資產匯出 CSV（14 欄、UTF-8 BOM；
 * 見 spec §4、§5）。
 */
export function downloadAssetsCsv(
  params: AssetExportParams = {}
): Promise<DownloadedFile> {
  return apiDownload(`/api/v1/assets/export${queryString(params)}`);
}

export function createAsset(input: AssetInput): Promise<Asset> {
  return apiPost<Asset>("/api/v1/assets", input);
}

/** 讀取單一資產詳情（含介面清單），供編輯對話框的介面子編輯器使用。 */
export function fetchAsset(id: number): Promise<AssetDetail> {
  return apiGet<AssetDetail>(`/api/v1/assets/${id}`);
}

export function updateAsset(id: number, input: AssetInput): Promise<Asset> {
  return apiPatch<Asset>(`/api/v1/assets/${id}`, input);
}

export function deleteAsset(id: number): Promise<void> {
  return apiDelete(`/api/v1/assets/${id}`);
}

/** 位置建議值：既有值去重、不分大小寫。 */
export function fetchLocations(): Promise<string[]> {
  return apiGet<StringItems>("/api/v1/locations").then(result => result.items);
}

/** 廠牌建議值：供篩選選單使用。 */
export function fetchBrands(): Promise<string[]> {
  return apiGet<StringItems>("/api/v1/brands").then(result => result.items);
}

/** 標籤建議值：所有已使用標籤去重（不分大小寫），供篩選與輸入建議。 */
export function fetchTags(): Promise<string[]> {
  return apiGet<StringItems>("/api/v1/tags").then(result => result.items);
}

/** 以設備序號精確查找，供重複提示（僅提示、不阻擋）。 */
export async function findByDeviceSerial(serial: string): Promise<Asset[]> {
  const page = await listAssets({ device_serial: serial, per_page: 200 });
  return page.items;
}
