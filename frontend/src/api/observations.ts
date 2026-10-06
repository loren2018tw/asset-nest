import { apiDownload, apiGet, type DownloadedFile } from "@/api/client";
import type { IpSeenSource } from "@/api/ips";

/** 觀測事件種類：首見、MAC 變更（見 GLOSSARY.md「觀測詞彙」、ADR-0016）。 */
export type ObservationEventKind = "first_seen" | "mac_changed";

/** 觀測事件（`observation_event` 一列；見票 06）。 */
export interface ObservationEvent {
  id: number;
  mac: string | null;
  kind: ObservationEventKind;
  source: IpSeenSource;
  observed_at: string;
}

/** 位址觀測現況；從未上線／未掃描時為 null 或欄位為 null。 */
export interface ObservationPresence {
  last_seen_at: string | null;
  last_seen_mac: string | null;
  last_seen_source: IpSeenSource | null;
  last_checked_at: string | null;
}

/** 觀測 MAC 連結到的資產摘要（已知 MAC；見票 06）。 */
export interface ObservationAsset {
  id: number;
  description: string;
  location: string;
  property_no: string | null;
}

/** 某位址用過的 MAC 彙總列。 */
export interface ObservedMac {
  mac: string;
  first_seen_at: string;
  last_seen_at: string;
  source: IpSeenSource | null;
  /** 是否為任一 Interface 的 MAC（不分大小寫）。 */
  known: boolean;
  /** 已知 MAC 的連結資產（第一筆命中）；未知時不帶此欄位。 */
  asset?: ObservationAsset;
}

/** IP 歷史：有效涵蓋＋現況＋事件（新到舊）＋用過的 MAC。 */
export interface IpObservationHistory {
  /** 有效觀測涵蓋（網段已開觀測且本機同 L2；據此區分未觀測／從未上線）。 */
  observed: boolean;
  presence: ObservationPresence | null;
  events: ObservationEvent[];
  macs: ObservedMac[];
}

/** MAC 歷史中的單一位址 sightings 列。 */
export interface MacSighting {
  address: string;
  first_seen_at: string;
  last_seen_at: string;
  source: IpSeenSource | null;
}

/** MAC 歷史：用過哪些位址＋是否已知（含連結資產）。 */
export interface MacObservationHistory {
  mac: string;
  known: boolean;
  asset?: ObservationAsset;
  sightings: MacSighting[];
}

/** 網段外觀測清單的一列（`GET /api/v1/observations/out-of-subnet`；見票 01、02）。 */
export interface OutOfSubnetObservation {
  /** 探測時所屬的受管網段（同一 L2 多個網段可能各有一列，見 ADR-0017）。 */
  subnet_id: number;
  subnet_cidr: string;
  subnet_name: string | null;
  address: string;
  /** 最後可見 MAC；無有效 MAC 為 null（被動列通常有值）。 */
  mac: string | null;
  /** 該列最早事件時間；事件經保留清理後退化為 `last_seen_at`。 */
  first_seen_at: string;
  last_seen_at: string;
  source: IpSeenSource | null;
  /** 是否為任一 Interface 的 MAC（不分大小寫）。 */
  known: boolean;
  /** 已知 MAC 的連結資產；未知時不帶此欄位。 */
  asset?: ObservationAsset;
}

/** 網段外觀測清單回應（後端已依 `last_seen_at` 新到舊排序）。 */
export interface OutOfSubnetObservationPage {
  items: OutOfSubnetObservation[];
}

/** 讀取單一 IP 的觀測歷史（現況、事件時間軸、用過的 MAC）。 */
export function fetchIpObservations(
  subnetId: number,
  address: string
): Promise<IpObservationHistory> {
  return apiGet<IpObservationHistory>(
    `/api/v1/subnets/${subnetId}/ips/${encodeURIComponent(address)}/observations`
  );
}

/** 以 MAC 讀取觀測歷史（用過哪些位址；不分大小寫）。 */
export function fetchMacObservations(
  mac: string
): Promise<MacObservationHistory> {
  return apiGet<MacObservationHistory>(
    `/api/v1/observations/mac/${encodeURIComponent(mac)}`
  );
}

/** 讀取被動監聽到的網段外位址清單（last_seen 新到舊；見票 01、02）。 */
export function listOutOfSubnetObservations(): Promise<OutOfSubnetObservationPage> {
  return apiGet<OutOfSubnetObservationPage>(
    "/api/v1/observations/out-of-subnet"
  );
}

/** 下載單一 IP 的觀測歷史 CSV（UTF-8 BOM；檔名由後端 Content-Disposition 提供）。 */
export function downloadIpObservationsCsv(
  subnetId: number,
  address: string
): Promise<DownloadedFile> {
  return apiDownload(
    `/api/v1/subnets/${subnetId}/ips/${encodeURIComponent(address)}/observations/export`
  );
}
