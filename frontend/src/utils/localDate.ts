/**
 * 本地時區的日期工具（見 `.scratch/asset-disposal/spec.md` §5）。
 */

/**
 * 本地時區的今天（`YYYY-MM-DD`）。
 * 勿用 `toISOString()`：UTC 與本地跨日時會得到錯誤日期。
 */
export function localDateString(): string {
  const now = new Date();
  const year = now.getFullYear();
  const month = String(now.getMonth() + 1).padStart(2, "0");
  const day = String(now.getDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
}
