import { apiDelete, apiGet, apiPatch, apiPost } from "@/api/client";

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
  /** 屆齡：購置日期＋年限早於今天（僅提示）。 */
  expired: boolean;
  created_at: string;
  updated_at: string;
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
}

/** 清單搜尋、篩選與分頁參數（皆為伺服器端）。 */
export interface AssetListParams {
  q?: string | undefined;
  location?: string | undefined;
  brand?: string | undefined;
  device_serial?: string | undefined;
  page?: number | undefined;
  per_page?: number | undefined;
}

export interface AssetPage {
  items: Asset[];
  total: number;
  page: number;
  per_page: number;
}

interface StringItems {
  items: string[];
}

export function listAssets(params: AssetListParams = {}): Promise<AssetPage> {
  const query = new URLSearchParams();
  for (const [key, value] of Object.entries(params)) {
    if (value !== undefined && value !== null && value !== "") {
      query.set(key, String(value));
    }
  }

  const search = query.toString();
  return apiGet<AssetPage>(
    `/api/v1/assets${search === "" ? "" : `?${search}`}`
  );
}

export function createAsset(input: AssetInput): Promise<Asset> {
  return apiPost<Asset>("/api/v1/assets", input);
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

/** 以設備序號精確查找，供重複提示（僅提示、不阻擋）。 */
export async function findByDeviceSerial(serial: string): Promise<Asset[]> {
  const page = await listAssets({ device_serial: serial, per_page: 200 });
  return page.items;
}
