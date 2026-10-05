/**
 * 觀測「最後可見」的相對時間顯示（見票 02、spec §前端）：
 * 剛看到／N 分鐘前／N 小時前／N 天前。
 */

const MINUTE_MS = 60 * 1000;
const HOUR_MS = 60 * MINUTE_MS;
const DAY_MS = 24 * HOUR_MS;

/**
 * 將 UTC 時間戳（`YYYY-MM-DDTHH:MM:SSZ`）轉為相對時間文字。
 * 解析失敗時原樣回傳，避免顯示錯誤資訊；未來時間（時鐘偏差）視為「剛看到」。
 */
export function relativeTime(value: string, now: Date = new Date()): string {
  const time = Date.parse(value);
  if (Number.isNaN(time)) {
    return value;
  }

  const elapsed = now.getTime() - time;
  if (elapsed < MINUTE_MS) {
    return "剛看到";
  }
  if (elapsed < HOUR_MS) {
    return `${Math.floor(elapsed / MINUTE_MS)} 分鐘前`;
  }
  if (elapsed < DAY_MS) {
    return `${Math.floor(elapsed / HOUR_MS)} 小時前`;
  }
  return `${Math.floor(elapsed / DAY_MS)} 天前`;
}
