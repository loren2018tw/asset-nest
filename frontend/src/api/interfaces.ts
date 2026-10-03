import { apiDelete, apiPatch, apiPost } from "@/api/client";

/** 語意警示：僅提示、不阻擋儲存（見 ADR-0006；後續票沿用）。 */
export interface Warning {
  code: string;
  message: string;
}

/** 網路介面（見 spec §2.2）。 */
export interface Interface {
  id: number;
  asset_id: number;
  name: string | null;
  /** 正規化為小寫冒號格式；`null`＝MAC 空白（手動設定介面）。 */
  mac: string | null;
  note: string | null;
  created_at: string;
  updated_at: string;
}

/** 新增／編輯介面內容；`null` 代表清除選填欄位。 */
export interface InterfaceInput {
  name: string | null;
  mac: string | null;
  note: string | null;
}

/** 介面儲存結果：介面欄位＋不阻擋的警示。 */
export interface InterfaceSaved extends Interface {
  warnings: Warning[];
}

/** 於資產底下新增介面。 */
export function createInterface(
  assetId: number,
  input: InterfaceInput
): Promise<InterfaceSaved> {
  return apiPost<InterfaceSaved>(`/api/v1/assets/${assetId}/interfaces`, input);
}

/** 編輯介面（未提供欄位維持原值、`null` 清除）。 */
export function updateInterface(
  id: number,
  input: InterfaceInput
): Promise<InterfaceSaved> {
  return apiPatch<InterfaceSaved>(`/api/v1/interfaces/${id}`, input);
}

/** 刪除介面（連動刪除指派，見 spec §2.2）。 */
export function deleteInterface(id: number): Promise<void> {
  return apiDelete(`/api/v1/interfaces/${id}`);
}
