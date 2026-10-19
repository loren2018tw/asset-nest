import type { QVueGlobals } from "quasar";

import { createAsset, updateAsset } from "@/api/assets";
import { assignFromAsset } from "@/api/assignments";
import {
  createInterface,
  deleteInterface,
  updateInterface
} from "@/api/interfaces";
import { assignIp, cancelAssignment, registerIp } from "@/api/ips";
import type { AssignedElsewhere, FlowDeps } from "@/utils/assignmentFlow";
import { assetLabel } from "@/utils/assetLabel";
import { notifyKeaSync } from "@/utils/keaSync";

/**
 * `FlowDeps` 的真 adapter（見 spec §介面）：`api` 綁 `@/api/*` 真函式、
 * `notify.warning` 為 `timeout: 6000` 警示、`notify.kea` 由 `notifyKeaSync`
 * 承接；`confirm`＝`$q` 移轉確認對話框。測試改傳 in-process fake；
 * 流程 module 本體不 import 本檔。
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

/** 移轉確認：顯示目前指派對象，確定後回傳 true。 */
function confirmTransfer(
  $q: QVueGlobals,
  addressText: string,
  target: AssignedElsewhere
): Promise<boolean> {
  const interfaceName = target.interface_name ?? "未命名";
  const mac = target.mac ?? "無 MAC";
  const assetText = assetLabel(
    target.asset_property_no,
    target.asset_description
  );

  return new Promise(resolve => {
    $q.dialog({
      title: "位址已指派",
      message:
        `位址 ${addressText} 目前已指派給「${assetText}（${target.asset_location}）」` +
        `的介面「${interfaceName}（${mac}）」。確定要將此指派移轉到目前介面？`,
      cancel: true,
      persistent: true
    })
      .onOk(() => resolve(true))
      .onCancel(() => resolve(false))
      .onDismiss(() => resolve(false));
  });
}
