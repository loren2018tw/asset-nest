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

interface ErrorBody {
  error?: string;
  message?: string;
}

async function request<T>(
  method: string,
  path: string,
  body?: unknown
): Promise<T> {
  const headers: Record<string, string> = { Accept: "application/json" };
  const init: RequestInit = { method, headers };
  if (body !== undefined) {
    headers["Content-Type"] = "application/json";
    init.body = JSON.stringify(body);
  }

  const response = await fetch(`${BASE_URL}${path}`, init);

  if (!response.ok) {
    throw new ApiError(
      response.status,
      await errorMessage(response, method, path)
    );
  }

  if (response.status === 204) {
    return undefined as T;
  }

  return (await response.json()) as T;
}

/** 優先採用後端 `{error, message}` 的訊息（見 spec §5）。 */
async function errorMessage(
  response: Response,
  method: string,
  path: string
): Promise<string> {
  try {
    const body = (await response.json()) as ErrorBody;
    if (body.message) {
      return body.message;
    }
  } catch {
    // 非 JSON 內容：改用狀態碼訊息
  }

  return `${method} ${path} 失敗（HTTP ${response.status}）`;
}

export function apiGet<T>(path: string): Promise<T> {
  return request<T>("GET", path);
}

export function apiPost<T>(path: string, body: unknown): Promise<T> {
  return request<T>("POST", path, body);
}

export function apiPatch<T>(path: string, body: unknown): Promise<T> {
  return request<T>("PATCH", path, body);
}

export function apiDelete(path: string): Promise<void> {
  return request<void>("DELETE", path);
}
