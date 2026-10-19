/**
 * 錯誤訊息收斂（見 `.scratch/assignment-flow/spec.md` §決策 7）：
 * `Error` 取 `message`，其餘以 `String` 轉換；原先 16 檔逐字重複同一段。
 */
export function messageOf(cause: unknown): string {
  return cause instanceof Error ? cause.message : String(cause);
}
