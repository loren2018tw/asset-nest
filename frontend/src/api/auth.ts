import { apiGet, apiPost } from "@/api/client";

/** 登入／工作階段回應：目前登入帳號（見 spec §4）。 */
export interface SessionResponse {
  username: string;
}

/** `POST /api/v1/login`：成功取得工作階段 cookie 並回傳帳號。 */
export function login(
  username: string,
  password: string
): Promise<SessionResponse> {
  return apiPost<SessionResponse>("/api/v1/login", { username, password });
}

/** `POST /api/v1/logout`：免登入、冪等，一律回 204（後端忽略 body）。 */
export function logout(): Promise<void> {
  return apiPost<void>("/api/v1/logout", {});
}

/** `GET /api/v1/session`：未登入由後端 middleware 回 401（見 spec §4）。 */
export function fetchSession(): Promise<SessionResponse> {
  return apiGet<SessionResponse>("/api/v1/session");
}
