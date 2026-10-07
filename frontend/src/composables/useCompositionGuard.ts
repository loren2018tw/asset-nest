import { onBeforeUnmount, watch, type Ref } from "vue";

/**
 * 注音符號（含聲調）為預編輯尾端字元：ㄅ–ㄩ、注音擴充區、輕聲與聲調符號。
 * 詳見 `.scratch/ime-composition/spec.md`。
 */
const BOPOMOFO_TAIL_RE =
  /[\u3105-\u312F\u31A0-\u31BF\u02C7\u02C9\u02CA\u02CB\u02D9]$/u;

/** 模板 ref 的最小介面：Vue 元件公開實例。 */
interface GuardTarget {
  $el?: unknown;
}

function textTailIsBopomofo(text: string): boolean {
  return BOPOMOFO_TAIL_RE.test(text);
}

function eventInputValue(event: Event): string {
  return event.target instanceof HTMLInputElement ? event.target.value : "";
}

const inputValueDesc = Object.getOwnPropertyDescriptor(
  HTMLInputElement.prototype,
  "value"
);

/**
 * 注音組字守衛。
 *
 * Quasar 2.34.0 的組字偵測只認日／中／韓字元，注音符號（U+3105–U+312F）
 * 不在範圍內；且實測 fcitx5-chewing + Chrome 的組字 session 跨詞長存、
 * 整段 focus 期間只在失焦時發出一次 compositionend，因此「組字中不 emit、
 * compositionend 再 flush」的框架流程在此平台上無法用於注音輸入——設起
 * `qComposing` 反而會讓已確認文字被扣住、直到失焦才更新。
 *
 * 守衛以「值尾端仍為注音字元」判定預編輯進行中，於控制項根元素以捕獲階段
 * 攔下兩個環節：
 * - `compositionupdate`：資料或欄位值尾端為注音時攔下，避免 Quasar 的
 *   偵測（含 Firefox 的泛用判斷）把 `qComposing` 設起。
 * - `input`：`isComposing` 且值尾端為注音（純預編輯）時攔下，組字中不寫
 *   model、不排程查詢。
 * 另於原生 input 上接管 `value` setter：預編輯進行中吞掉程式化寫入。
 * Vue 重繪（`patchDOMProp`）或框架把 model 寫回 DOM 時，若值尾端仍是注音，
 * 寫入會蓋掉預編輯並使 Chromium 重設 IME（實測：組字被中斷、字串錯亂）；
 * 瀏覽器與 IME 自身的編輯不走 JS setter，不受影響。
 *
 * 已確認文字（值尾端非注音）照常通過、走既有 debounce 查詢；注意本平台
 * commit 事件的 `isComposing` 亦為 true，值尾端才是判別依據，應用端其他
 * input 處理不得以 `isComposing` 跳過，否則會漏掉 commit。其他語言的 IME
 * 不受影響（資料與值尾端皆非注音即不攔）。
 */
export function useCompositionGuard(
  target: Ref<GuardTarget | null | undefined>
): void {
  let root: Element | null = null;
  let input: HTMLInputElement | null = null;

  function onCompositionUpdateCapture(event: Event): void {
    if (!(event instanceof CompositionEvent)) return;
    const data = typeof event.data === "string" ? event.data : "";
    if (
      textTailIsBopomofo(data) ||
      textTailIsBopomofo(eventInputValue(event))
    ) {
      event.stopPropagation();
    }
  }

  function onInputCapture(event: Event): void {
    if (!(event instanceof InputEvent) || event.isComposing !== true) return;
    if (textTailIsBopomofo(eventInputValue(event))) {
      event.stopPropagation();
    }
  }

  function detach(): void {
    if (root !== null) {
      root.removeEventListener(
        "compositionupdate",
        onCompositionUpdateCapture,
        true
      );
      root.removeEventListener("input", onInputCapture, true);
      root = null;
    }
    if (input !== null) {
      Reflect.deleteProperty(input, "value");
      input = null;
    }
  }

  function attach(): void {
    detach();
    const el = target.value?.$el;
    if (!(el instanceof Element)) return;

    root = el;
    el.addEventListener("compositionupdate", onCompositionUpdateCapture, true);
    el.addEventListener("input", onInputCapture, true);

    const nativeInput = el.querySelector("input");
    if (nativeInput !== null && inputValueDesc !== undefined) {
      input = nativeInput;
      Object.defineProperty(nativeInput, "value", {
        configurable: true,
        get() {
          return inputValueDesc.get?.call(this);
        },
        set(value: string) {
          if (textTailIsBopomofo(String(inputValueDesc.get?.call(this) ?? "")))
            return;
          inputValueDesc.set?.call(this, value);
        }
      });
    }
  }

  watch(target, attach, { flush: "post" });
  onBeforeUnmount(detach);
}
