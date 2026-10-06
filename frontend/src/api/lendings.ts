import { apiGet, apiPost } from "@/api/client";

/**
 * 借還（Lending）API（見 `.scratch/asset-lending/spec.md` §3、§4）。
 *
 * 型別欄位採全站慣例：直接對映後端 JSON 的 snake_case（`lent_at`、`due_at`
 * 等），不另做 camelCase 轉換（見票 02 偏離說明）。
 */

/** 借出紀錄（見 spec §3）。 */
export interface Lending {
  id: number;
  asset_id: number;
  borrower: string;
  /** 借出時間：伺服器當下（UTC ISO8601）。 */
  lent_at: string;
  /** 預計歸還日：`YYYY-MM-DD`，選填。 */
  due_at: string | null;
  /** 備註：自由文字，選填。 */
  note: string | null;
  /** 歸還時間（UTC ISO8601）；`null`＝出借中。 */
  returned_at: string | null;
  /** 逾期：未歸還且預計歸還日早於今天（後端判定）。 */
  overdue: boolean;
}

/** 列表用的借出紀錄：附加資產編號與描述（見 spec §3）。 */
export interface LendingWithAsset extends Lending {
  property_no: string | null;
  description: string;
}

/** 資產清單列附帶的出借中摘要（未出借為 null；見 spec §4、§5）。 */
export interface LendingBrief {
  id: number;
  borrower: string;
  lent_at: string;
  due_at: string | null;
}

/** 新增借出的輸入；`null` 代表不填選填欄位。 */
export interface LendingInput {
  borrower: string;
  due_at: string | null;
  note: string | null;
}

/** 已歸還紀錄分頁參數（`page` 由 1 起、`per_page` 預設 10；見 spec §4）。 */
export interface LendingListParams {
  page?: number | undefined;
  per_page?: number | undefined;
}

/** 已歸還紀錄分頁回應。 */
export interface LendingPage {
  items: LendingWithAsset[];
  total: number;
  page: number;
  per_page: number;
}

/** 將參數轉為查詢字串（不含前綴）；略過 undefined。 */
function queryString(params: object): string {
  const query = new URLSearchParams();
  for (const [key, value] of Object.entries(params)) {
    if (value !== undefined && value !== null) {
      query.set(key, String(value));
    }
  }
  return query.toString();
}

/** 建立借出：回 201＋`Lending`；已出借中回 409（見 spec §4）。 */
export function createLending(
  assetId: number,
  input: LendingInput
): Promise<Lending> {
  return apiPost<Lending>(`/api/v1/assets/${assetId}/lendings`, input);
}

/** 歸還：記歸還時間、回 200＋`Lending`；已歸還回 409（見 spec §4）。 */
export function returnLending(id: number): Promise<Lending> {
  return apiPost<Lending>(`/api/v1/lendings/${id}/return`, {});
}

/** 出借中清單（不分頁、借出時間倒序；見 spec §4）。 */
export function listOpenLendings(): Promise<LendingWithAsset[]> {
  return apiGet<{ items: LendingWithAsset[] }>(
    "/api/v1/lendings?returned=false"
  ).then(result => result.items);
}

/** 已歸還紀錄（伺服器分頁、借出時間倒序；見 spec §4）。 */
export function listReturnedLendings(
  params: LendingListParams = {}
): Promise<LendingPage> {
  const query = queryString({ returned: true, ...params });
  return apiGet<LendingPage>(`/api/v1/lendings?${query}`);
}

/** 借用人建議值：既有借用人去重，最近使用者在前（見 spec §4）。 */
export function listBorrowers(): Promise<string[]> {
  return apiGet<{ items: string[] }>("/api/v1/lendings/borrowers").then(
    result => result.items
  );
}
