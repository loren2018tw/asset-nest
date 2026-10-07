export interface Debounced {
  /** 排程呼叫；delay 內再次呼叫會重新計時。 */
  call(): void;
  /** 取消尚未執行的呼叫（供卸載時清理）。 */
  cancel(): void;
}

/** 最小去抖：`.call()` 於 delay 毫秒內無新呼叫後執行一次。 */
export function debounce(fn: () => void, delayMs: number): Debounced {
  let timer: number | null = null;

  return {
    call() {
      if (timer !== null) window.clearTimeout(timer);
      timer = window.setTimeout(() => {
        timer = null;
        fn();
      }, delayMs);
    },
    cancel() {
      if (timer !== null) {
        window.clearTimeout(timer);
        timer = null;
      }
    }
  };
}
