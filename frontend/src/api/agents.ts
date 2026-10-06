import { apiGet } from "@/api/client";

/** 觀測代理現況（`GET /api/v1/agents` 的一列；見票 01）。 */
export interface Agent {
  /** 代理 `instance_id`。 */
  instance_id: string;
  name: string;
  version: string;
  /** 連線來源 IP（忽略 X-Forwarded-For）。 */
  source_ip: string;
  /** 代理回報並正規化的涵蓋 CIDR。 */
  subnet_cidr: string;
  /** 精確對應的受管網段；未對應（或網段已刪除）為 null。 */
  subnet_id: number | null;
  subnet_name: string | null;
  first_report_at: string;
  last_report_at: string;
  /** 最後一次觀測回報；本階段尚未寫入（見票 02）。 */
  last_observation_at: string | null;
  /** `last_report_at` 在後端 `AGENT_STALE_SECS` 門檻內。 */
  online: boolean;
}

/** 代理清單回應（後端已依 `last_report_at` 新到舊排序）。 */
export interface AgentList {
  /** 在線門檻秒數（後端 `AGENT_STALE_SECS`）。 */
  stale_secs: number;
  items: Agent[];
}

/** 被拒回報（`agent_auth_failure` 一列；同來源 IP 彙總；見 ADR-0019）。 */
export interface AgentAuthFailure {
  source_ip: string;
  /** 自報名稱；body 解析失敗或未帶為 null。 */
  claimed_name: string | null;
  claimed_version: string | null;
  first_attempt_at: string;
  last_attempt_at: string;
  attempt_count: number;
}

/** 被拒回報清單回應（後端已依 `last_attempt_at` 新到舊排序）。 */
export interface AgentAuthFailureList {
  items: AgentAuthFailure[];
}

/** 讀取觀測代理清單（`last_report_at` 新到舊）。 */
export function listAgents(): Promise<AgentList> {
  return apiGet<AgentList>("/api/v1/agents");
}

/** 讀取被拒回報清單（`last_attempt_at` 新到舊）。 */
export function listAgentAuthFailures(): Promise<AgentAuthFailureList> {
  return apiGet<AgentAuthFailureList>("/api/v1/agents/auth-failures");
}
