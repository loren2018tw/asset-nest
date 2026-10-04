import { apiPut } from "@/api/client";
import type { Warning } from "@/api/interfaces";
import type { Assignment, IpPurpose } from "@/api/ips";

/** 資產端指派輸入（見票 10、ADR-0007）。 */
export interface AssetAssignmentInput {
  /** 完整 IP（v4／v6）；由後端反推所屬網段。 */
  address: string;
  interface_id: number;
  purpose: IpPurpose;
  /** 僅保留用途可填。 */
  hostname: string | null;
  /** 位址已指派給其他介面時，是否確認移轉。 */
  transfer: boolean;
}

/** 資產端指派結果：指派欄位＋不阻擋的警示＋是否發生移轉。 */
export interface AssetAssignmentSaved extends Assignment {
  warnings: Warning[];
  /** 是否發生移轉（原指派已取消、位址改派給目前介面）。 */
  transferred: boolean;
}

/** 從資產端指派 IP；已指派給其他介面時先以 `transfer: false` 取得提示，
 *  確認後以 `transfer: true` 重試（見 ADR-0007）。 */
export function assignFromAsset(
  assetId: number,
  input: AssetAssignmentInput
): Promise<AssetAssignmentSaved> {
  return apiPut<AssetAssignmentSaved>(
    `/api/v1/assets/${assetId}/assignments`,
    input
  );
}
