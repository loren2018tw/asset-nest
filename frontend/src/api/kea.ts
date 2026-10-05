import { apiGet, apiPost } from "@/api/client";

/** 單筆推送結果（隨指派／取消指派回應附帶；僅在應同步時出現，見 ADR-0011）。 */
export interface KeaSyncState {
  status: "ok" | "failed";
  message?: string | null;
}

/** 保留的可比對欄位。 */
export interface KeaRecordFields {
  hw_address: string;
  hostname: string | null;
}

/** 計畫中的一筆變更：新增＝desired、刪除＝current、更新＝兩者。 */
export interface KeaPlanItem {
  ip_address: string;
  desired?: KeaRecordFields;
  current?: KeaRecordFields;
}

/** 被跳過的項目（語意衝突、非 hw-address 形式的 Kea 保留）。 */
export interface KeaPlanSkip {
  ip_address?: string | null;
  reason: string;
}

export interface KeaPlanSubnet {
  /** asset-nest 的網段 id。 */
  subnet_id: number;
  cidr: string;
  name: string | null;
  kea_subnet_id: number;
  add: KeaPlanItem[];
  update: KeaPlanItem[];
  delete: KeaPlanItem[];
  skipped: KeaPlanSkip[];
  error?: string | null;
}

export interface KeaSyncPlan {
  subnets: KeaPlanSubnet[];
  totals: { add: number; update: number; delete: number; skipped: number };
}

export interface KeaApplyFailure {
  action: "add" | "update" | "delete";
  ip_address: string;
  message: string;
}

export interface KeaApplySubnet {
  subnet_id: number;
  cidr: string;
  name: string | null;
  kea_subnet_id: number;
  added: number;
  updated: number;
  deleted: number;
  skipped: number;
  failures: KeaApplyFailure[];
  error?: string | null;
}

export interface KeaApplyReport {
  subnets: KeaApplySubnet[];
  config_write: "ok" | "failed" | "skipped";
  config_write_message?: string | null;
}

/** 完整同步計畫（dry-run、唯讀）。 */
export function getSyncPlan(): Promise<KeaSyncPlan> {
  return apiGet<KeaSyncPlan>("/api/v1/kea/sync/plan");
}

/** 重算計畫並套用完整同步（見 ADR-0011）。 */
export function applySync(): Promise<KeaApplyReport> {
  return apiPost<KeaApplyReport>("/api/v1/kea/sync", {});
}
