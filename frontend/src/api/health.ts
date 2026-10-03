import { apiGet } from "@/api/client";

export interface HealthResponse {
  status: string;
  service: string;
  version: string;
  database: string;
}

export function fetchHealth(): Promise<HealthResponse> {
  return apiGet<HealthResponse>("/api/health");
}
