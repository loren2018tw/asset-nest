import type { QVueGlobals } from "quasar";

import type { KeaSyncState } from "@/api/kea";

/** Kea 單筆推送失敗時以 warning 提示（沿既有警示慣例）；成功或未涉同步不提示。 */
export function notifyKeaSync(
  $q: QVueGlobals,
  sync?: KeaSyncState | null
): void {
  if (sync?.status === "failed") {
    $q.notify({
      type: "warning",
      message: `Kea 同步失敗：${sync.message ?? "未知錯誤"}`,
      timeout: 6000
    });
  }
}
