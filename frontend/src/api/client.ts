/**
 * 極簡 API client：預設使用相對路徑 `/api`（見 docs/adr/0004）。
 * 需要獨立部署前端時，可用 `VITE_API_BASE_URL` 覆寫。
 */
const BASE_URL = import.meta.env.VITE_API_BASE_URL ?? "";

export class ApiError extends Error {
  constructor(
    readonly status: number,
    message: string,
    /** 後端錯誤 body 的 `details`（結構錯誤的額外資訊；見 spec §5）。 */
    readonly details?: Record<string, unknown>
  ) {
    super(message);
    this.name = "ApiError";
  }
}

interface ErrorBody {
  error?: string;
  message?: string;
  details?: Record<string, unknown>;
}

/** 實際送出請求：統一 `Accept`、錯誤解析與 204／JSON 處理。 */
async function send<T>(
  method: string,
  path: string,
  body?: BodyInit,
  contentType?: string
): Promise<T> {
  const headers: Record<string, string> = { Accept: "application/json" };
  if (contentType !== undefined) {
    headers["Content-Type"] = contentType;
  }
  const init: RequestInit = { method, headers };
  if (body !== undefined) {
    init.body = body;
  }

  const response = await fetch(`${BASE_URL}${path}`, init);

  if (!response.ok) {
    const error = await parseError(response, method, path);
    throw new ApiError(response.status, error.message, error.details);
  }

  if (response.status === 204) {
    return undefined as T;
  }

  return (await response.json()) as T;
}

async function request<T>(
  method: string,
  path: string,
  body?: unknown
): Promise<T> {
  return send<T>(
    method,
    path,
    body === undefined ? undefined : JSON.stringify(body),
    body === undefined ? undefined : "application/json"
  );
}

/** 優先採用後端 `{error, message}` 的訊息（見 spec §5），並保留 `details`。 */
async function parseError(
  response: Response,
  method: string,
  path: string
): Promise<{ message: string; details: Record<string, unknown> | undefined }> {
  const fallback = `${method} ${path} 失敗（HTTP ${response.status}）`;

  try {
    const body = (await response.json()) as ErrorBody;
    return {
      message: body.message || fallback,
      details: body.details
    };
  } catch {
    // 非 JSON 內容：改用狀態碼訊息
    return { message: fallback, details: undefined };
  }
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

export function apiPut<T>(path: string, body: unknown): Promise<T> {
  return request<T>("PUT", path, body);
}

export function apiDelete(path: string): Promise<void> {
  return request<void>("DELETE", path);
}

/**
 * multipart/form-data 上傳；檔案欄位固定為 `file`。
 * 不手動設定 `Content-Type`，由瀏覽器帶上 multipart boundary；
 * 錯誤解析沿用 `{error,message,details}` 與 `ApiError`。
 */
export function apiUpload<T>(path: string, file: File): Promise<T> {
  const form = new FormData();
  form.append("file", file);
  return send<T>("POST", path, form);
}
