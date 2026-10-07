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

/** gateway（routers option）變更；null＝未設／移除（見 ADR-0013）。 */
export interface KeaGatewayPlan {
  current: string | null;
  desired: string | null;
}

/** 將建立的 Kea 網段內容（見 ADR-0023）。 */
export interface KeaSubnetAddPlan {
  /** 期望 pool 範圍（正規化 `start-end`、數值排序）。 */
  pools: string[];
  /** 期望 gateway（routers option）；未設為 null。 */
  gateway: string | null;
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
  /** 要新增的 pool 範圍（正規化 `start-end`）。 */
  pool_add: string[];
  /** 要刪除的 pool 範圍（正規化 `start-end`）。 */
  pool_delete: string[];
  /** gateway 變更；相同時省略。 */
  gateway?: KeaGatewayPlan | null;
  /** 要建立的 Kea 網段；Kea 查無該 id 且無相同 CIDR 時出現（見 ADR-0023）。 */
  subnet_add?: KeaSubnetAddPlan | null;
  error?: string | null;
}

export interface KeaSyncPlan {
  subnets: KeaPlanSubnet[];
  totals: {
    add: number;
    update: number;
    delete: number;
    skipped: number;
    /** 要新增的 pool 筆數。 */
    pool_add: number;
    /** 要刪除的 pool 筆數。 */
    pool_delete: number;
    /** 要變更 gateway 的網段數。 */
    gateway: number;
    /** 要建立的 Kea 網段筆數（見 ADR-0023）。 */
    subnet_add: number;
  };
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
  /** 成功新增的 pool 筆數。 */
  pool_added: number;
  /** 成功刪除的 pool 筆數。 */
  pool_deleted: number;
  /** gateway 是否已更新。 */
  gateway_updated: boolean;
  /** 是否已建立 Kea 網段（見 ADR-0023）。 */
  subnet_added: boolean;
  /** 建立 Kea 網段失敗訊息；成功時省略。 */
  subnet_add_error?: string | null;
  /** 網段層（pool／gateway）套用失敗訊息；成功時省略。 */
  settings_error?: string | null;
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

/** `version-get` 回應區塊（見票 01）。 */
export interface KeaVersionBlock {
  /** 回應 `arguments.version`；真機 3.2.1 未提供為 null。 */
  version: string | null;
  /** 回應 `text`（如 "3.2.1"）；未提供為 null。 */
  text: string | null;
}

/** `status-get` 的 socket 狀態（真機 3.2.1 為物件 `{"status":"ready"}`）。 */
export interface KeaSocketStatus {
  /** socket 狀態（如 `ready`）；未提供為 null。 */
  status: string | null;
}

/** `status-get` 的伺服器運行資訊；`uptime`／`reload` 皆為相對秒數（非 epoch）。 */
export interface KeaRuntimeInfo {
  pid: number | null;
  /** 伺服器啟動後經過秒數。 */
  uptime: number | null;
  /** 距上次設定重載秒數。 */
  reload: number | null;
  /** socket 狀態；伺服端未提供為 null。 */
  sockets: KeaSocketStatus | null;
}

/** `config-get` 的 DHCPv4 摘要（見票 01）。 */
export interface KeaDhcp4Block {
  /** Kea `Dhcp4.subnet4` 筆數。 */
  subnet_count: number;
  /** 本地 `kea_subnet_id IS NOT NULL` 的網段數。 */
  managed_subnet_count: number;
  /** 租約庫類型（如 `memfile`）；未提供為 null。 */
  lease_backend: string | null;
}

/** 各命令獨立的失敗訊息；全成功時後端省略 `errors`。 */
export interface KeaStatusErrors {
  version?: string;
  config?: string;
  status?: string;
}

/** Kea 系統狀態回應（一律 200、分區容錯；見票 01）。 */
export interface KeaStatus {
  configured: boolean;
  reachable: boolean;
  url: string | null;
  version: KeaVersionBlock | null;
  /** 監聽介面（設定值）；空陣列＝未監聽、null＝無法取得。 */
  interfaces: string[] | null;
  runtime: KeaRuntimeInfo | null;
  dhcp4: KeaDhcp4Block | null;
  errors?: KeaStatusErrors;
}

/** 單筆 Kea DHCPv4 動態租約（見票 02）。 */
export interface KeaLease {
  ip_address: string | null;
  hw_address: string | null;
  hostname: string | null;
  subnet_id: number | null;
  /** 本地受管網段（`kea_subnet_id`）對應的 CIDR；無對應為 null。 */
  subnet_cidr: string | null;
  /** 本地受管網段名稱；無對應或未命名為 null。 */
  subnet_name: string | null;
  /** `cltt + valid_lft`（ISO 8601 UTC）；缺欄位為 null。 */
  expires_at: string | null;
  /**
   * 狀態：`default`／`declined`／`expired-reclaimed`／`released`／`registered`
   * （Kea 3.2 定義），未知保留原值。
   */
  state: string | null;
  /**
   * 本地「保留」：受管網段內、位址與 `purpose=reservation` 指派完全相符；
   * 無對應受管網段一律 false。
   */
  is_reservation: boolean;
}

/** Kea 系統狀態（唯讀診斷；見票 01）。 */
export function getKeaStatus(): Promise<KeaStatus> {
  return apiGet<KeaStatus>("/api/v1/kea/status");
}

interface KeaLeaseItems {
  leases: KeaLease[];
}

/** Kea DHCPv4 動態租約清單（唯讀；見票 02）。 */
export function listKeaLeases(): Promise<KeaLease[]> {
  return apiGet<KeaLeaseItems>("/api/v1/kea/leases").then(
    result => result.leases
  );
}
