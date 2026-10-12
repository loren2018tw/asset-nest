import type { Asset } from "@/api/assets";
import { apiGet, apiPost } from "@/api/client";
import type { KeaSyncState } from "@/api/kea";

/**
 * 資產報廢（Disposal）API（見 `.scratch/asset-disposal/spec.md` §5）。
 *
 * 型別欄位採全站慣例：直接對映後端 JSON 的 snake_case（`disposal_date`、
 * `asset_id` 等），不另做 camelCase 轉換。
 */

/** 報廢資產（見 spec §3、§5）。 */
export interface DisposedAsset {
  /** 報廢列自身的 id（復原以此為鍵）。 */
  id: number;
  /** 原資產 id；復原時沿用。 */
  asset_id: number;
  property_no: string | null;
  description: string;
  location: string;
  device_serial: string | null;
  brand: string | null;
  model: string | null;
  purchase_date: string | null;
  lifespan_years: number | null;
  note: string | null;
  /** 標籤：多值自由文字。 */
  tags: string[];
  /** 報廢日期：`YYYY-MM-DD`。 */
  disposal_date: string;
  /** 原資產的建立時間（搬移沿用）。 */
  created_at: string;
  /** 原資產的更新時間（搬移沿用）。 */
  updated_at: string;
}

/** 報廢輸入；報廢日期必填、`YYYY-MM-DD`、不可為未來。 */
export interface DisposeInput {
  disposal_date: string;
}

/** 報廢回應：報廢欄位攤平＋Kea 推送結果（僅涉及同步的保留逐筆；見 spec §4）。 */
export interface DisposeResponse extends DisposedAsset {
  kea_sync: KeaSyncState[];
}

/** 報廢清單分頁參數（`page` 由 1 起、`per_page` 預設 10；見 spec §4）。 */
export interface DisposedAssetListParams {
  q?: string | undefined;
  page?: number | undefined;
  per_page?: number | undefined;
}

/** 報廢清單分頁回應。 */
export interface DisposedAssetPage {
  items: DisposedAsset[];
  total: number;
  page: number;
  per_page: number;
}

/** 將參數轉為查詢字串（含 `?`）；略過 undefined／null／空字串（比照 api/assets.ts）。 */
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

/** 報廢資產：201＋`DisposeResponse`；出借中回 409（見 spec §4）。 */
export function disposeAsset(
  assetId: number,
  input: DisposeInput
): Promise<DisposeResponse> {
  return apiPost<DisposeResponse>(`/api/v1/assets/${assetId}/disposal`, input);
}

/** 報廢清單（伺服器分頁、報廢日期倒序；見 spec §4）。 */
export function listDisposedAssets(
  params: DisposedAssetListParams = {}
): Promise<DisposedAssetPage> {
  return apiGet<DisposedAssetPage>(
    `/api/v1/disposed-assets${queryString(params)}`
  );
}

/** 復原報廢資產：200＋`Asset`；不存在回 404（見 spec §4）。 */
export function restoreDisposedAsset(id: number): Promise<Asset> {
  return apiPost<Asset>(`/api/v1/disposed-assets/${id}/restore`, {});
}
