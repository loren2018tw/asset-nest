import { apiGet } from "@/api/client";

export interface PeerMacResponse {
  /** 查不到時為 `null`（需與本系統同一層網路；不做本機網卡 fallback）。 */
  mac: string | null;
}

/** 查詢目前連線主機 MAC，供新增介面時參考填入（見票 09）。 */
export function fetchPeerMac(): Promise<PeerMacResponse> {
  return apiGet<PeerMacResponse>("/api/v1/peer-mac");
}
