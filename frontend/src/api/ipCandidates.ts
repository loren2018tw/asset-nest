import { apiGet } from "@/api/client";

/** 指派候選：跨網段搜尋到的可用位址（見 spec §8）。 */
export interface IpCandidate {
  address: string;
  subnet_id: number;
  subnet_cidr: string;
  subnet_name: string | null;
}

/** 完整 v4 位址的狀態種類（見 spec §8）。 */
export type IpQueryStatusKind =
  | "available"
  | "in_pool"
  | "excluded"
  | "static"
  | "reservation"
  | "out_of_subnet";

/** 查詢位址狀態；`q` 非完整 v4 位址時後端回傳 `null`（見 spec §8）。 */
export interface IpQueryStatus {
  address: string;
  status: IpQueryStatusKind;
  subnet_id: number | null;
  subnet_cidr: string | null;
  subnet_name: string | null;
}

/** 候選查詢回應（無 `total`；見 spec §8）。 */
export interface IpCandidatesResult {
  items: IpCandidate[];
  query_status: IpQueryStatus | null;
}

/**
 * 依前綴搜尋跨網段可用位址（見 spec §8）。`q` 至少須有 2 個完整 v4 octet，
 * 否則後端回 400（呼叫端以 `parseIpv4Prefix` 把關）。
 */
export function findIpCandidates(
  q: string,
  limit = 20
): Promise<IpCandidatesResult> {
  const query = new URLSearchParams({ q, limit: String(limit) });
  return apiGet<IpCandidatesResult>(
    `/api/v1/ip-candidates?${query.toString()}`
  );
}
