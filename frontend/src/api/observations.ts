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

/** 下載單一 IP 的觀測歷史 CSV（UTF-8 BOM；檔名由後端 Content-Disposition 提供）。 */
export function downloadIpObservationsCsv(
  subnetId: number,
  address: string
): Promise<DownloadedFile> {
  return apiDownload(
    `/api/v1/subnets/${subnetId}/ips/${encodeURIComponent(address)}/observations/export`
  );
}
