/**
 * 極簡 API client：預設使用相對路徑 `/api`（見 docs/adr/0004）。
 * 需要獨立部署前端時，可用 `VITE_API_BASE_URL` 覆寫。
 */
const BASE_URL = import.meta.env.VITE_API_BASE_URL ?? "";

export class ApiError extends Error {
  constructor(
    readonly status: number,
    message: string
  ) {
    super(message);
    this.name = "ApiError";
  }
}

export async function apiGet<T>(path: string): Promise<T> {
  const response = await fetch(`${BASE_URL}${path}`, {
    headers: { Accept: "application/json" }
  });

  if (!response.ok) {
    throw new ApiError(
      response.status,
      `GET ${path} 失敗（HTTP ${response.status}）`
    );
  }

  return (await response.json()) as T;
}
