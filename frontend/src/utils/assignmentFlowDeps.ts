import type { QVueGlobals } from "quasar";

import { createAsset, updateAsset } from "@/api/assets";
import { assignFromAsset } from "@/api/assignments";
import {
  createInterface,
  deleteInterface,
  updateInterface
} from "@/api/interfaces";
import { assignIp, cancelAssignment, registerIp } from "@/api/ips";
import { confirmTransfer } from "@/utils/assignTransfer";
import type { FlowDeps } from "@/utils/assignmentFlow";
import { notifyKeaSync } from "@/utils/keaSync";

/**
 * `FlowDeps` 的真 adapter（見 spec §介面）：`api` 綁 `@/api/*` 真函式、
 * `notify.warning` 為 `timeout: 6000` 警示、`notify.kea` 由 `notifyKeaSync`
 * 承接；`confirm` 暫由 `assignTransfer.confirmTransfer` 轉接（票 06 移入）。
 * 測試改傳 in-process fake；流程 module 本體不 import 本檔。
 */
export function assignmentFlowDeps($q: QVueGlobals): FlowDeps {
  return {
    api: {
      createAsset,
      updateAsset,
      createInterface,
      updateInterface,
      deleteInterface,
      assignFromAsset,
      assignIp,
      registerIp,
      cancelAssignment
    },
    confirm: (address, target) => confirmTransfer($q, address, target),
    notify: {
      warning(message: string) {
        $q.notify({ type: "warning", message, timeout: 6000 });
      },
      kea(sync) {
        notifyKeaSync($q, sync);
      }
    }
  };
}
