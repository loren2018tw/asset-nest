<template>
  <q-dialog v-model="open" persistent>
    <q-card
      style="width: 560px; max-width: 95vw; max-height: 90vh; overflow: auto"
    >
      <q-form @submit="submit">
        <q-card-section>
          <div class="text-h6">{{ dialogTitle }}</div>
          <div class="text-subtitle2 text-grey-7">{{ subnetCidr }}</div>
        </q-card-section>

        <q-card-section class="q-gutter-y-sm">
          <q-banner
            v-if="errorMessage !== ''"
            dense
            rounded
            class="bg-negative text-white"
          >
            {{ errorMessage }}
          </q-banner>

          <!-- v6 新增：輸入位址（基本格式驗證）；其餘情境位址唯讀不可修改 -->
          <q-input
            v-if="addressEditable"
            v-model="newAddress"
            outlined
            dense
            label="IPv6 位址 *"
            :hint="`須落在 ${subnetCidr} 內`"
            :rules="[ipv6Rule]"
            lazy-rules
          />
          <q-input
            v-else
            :model-value="address"
            outlined
            dense
            readonly
            label="IP 位址（不可修改）"
          />

          <!-- 已指派：指派對象唯讀顯示；介面不可更換（換介面＝取消後重新指派） -->
          <template v-if="isEdit">
            <q-input
              :model-value="existingAssetLabel"
              outlined
              dense
              readonly
              label="資產"
            />
            <q-input
              :model-value="existingInterfaceLabel"
              outlined
              dense
              readonly
              label="介面"
            />
          </template>

          <!-- 未指派：搜尋資產 → 選介面（或當場新增） -->
          <template v-else>
            <q-select
              ref="assetSelectRef"
              v-model="selectedAsset"
              :options="assetOptions"
              :option-label="assetOptionLabel"
              :loading="searchingAssets"
              use-input
              input-debounce="300"
              outlined
              dense
              label="搜尋資產 *"
              hint="以描述／財產編號／設備序號等關鍵字搜尋"
              @filter="onFilterAssets"
              @update:model-value="onAssetSelected"
            >
              <template #option="scope">
                <q-item v-bind="scope.itemProps">
                  <q-item-section>
                    <q-item-label>{{
                      assetOptionLabel(scope.opt)
                    }}</q-item-label>
                    <q-item-label caption>{{
                      scope.opt.location
                    }}</q-item-label>
                  </q-item-section>
                </q-item>
              </template>
              <template #no-option>
                <q-item>
                  <q-item-section class="text-grey-6">
                    沒有符合的資產
                  </q-item-section>
                </q-item>
              </template>
            </q-select>

            <q-select
              v-model="selectedInterfaceId"
              :options="interfaceOptions"
              :loading="loadingInterfaces"
              :disable="selectedAsset === null"
              emit-value
              map-options
              outlined
              dense
              label="介面 *"
              :hint="
                selectedAsset !== null && interfaces.length === 0
                  ? '此資產尚無介面，請新增介面'
                  : undefined
              "
            />

            <new-interface-form
              ref="newInterfaceFormRef"
              :key="newInterfaceFormKey"
              :asset-id="selectedAsset?.id ?? null"
              :disabled="selectedAsset === null"
              @created="onInterfaceCreated"
            />
          </template>

          <!-- v6 恆為手動設定：無保留選項、無 hostname（見 spec §7） -->
          <q-input
            v-if="isV6"
            model-value="手動設定（static）"
            outlined
            dense
            readonly
            label="用途"
            hint="IPv6 不經 Kea；無保留與 pool 概念"
          />
          <template v-else>
            <q-option-group
              v-model="purpose"
              :options="purposeOptions"
              inline
              class="q-mt-sm"
            />
            <div
              v-if="selectedMac === null && !isEdit"
              class="text-caption text-grey-7"
            >
              所選介面無 MAC，不可設為保留（僅能手動設定）。
            </div>

            <q-input
              v-if="purpose === 'reservation'"
              v-model="hostname"
              outlined
              dense
              label="hostname"
              hint="選填；供 Kea 固定配發使用"
            />
          </template>
        </q-card-section>

        <q-card-actions align="right">
          <q-btn
            v-if="isEdit"
            flat
            color="negative"
            label="取消指派"
            :loading="cancelling"
            @click="confirmCancel"
          />
          <q-space />
          <q-btn v-close-popup flat label="關閉" />
          <!-- v6 編輯無可變欄位（換介面＝取消＋重新指派，見 ADR-0005）：僅供檢視與取消 -->
          <q-btn
            v-if="!isV6 || !isEdit"
            color="primary"
            type="submit"
            :label="submitLabel"
            :loading="saving"
          />
        </q-card-actions>
      </q-form>
    </q-card>
  </q-dialog>
</template>

<script setup lang="ts">
import { useQuasar, type QSelect } from "quasar";
import { computed, ref, watch } from "vue";

import { fetchAsset, listAssets, type Asset } from "@/api/assets";
import type { Interface, InterfaceSaved } from "@/api/interfaces";
import type { IpEntry, IpPurpose } from "@/api/ips";
import type { AddressFamily } from "@/api/subnets";
import NewInterfaceForm from "@/components/NewInterfaceForm.vue";
import { useCompositionGuard } from "@/composables/useCompositionGuard";
import {
  assignFromSubnetFlow,
  cancelAssignmentConfirmMessage,
  cancelAssignmentFlow,
  interfaceLabel,
  isIpv6,
  purposeOptionsFor,
  registerIpv6Flow
} from "@/utils/assignmentFlow";
import { assignmentFlowDeps } from "@/utils/assignmentFlowDeps";
import { assetLabel } from "@/utils/assetLabel";
import { messageOf } from "@/utils/errors";
import { cancelAssignmentHint } from "@/utils/observationHint";

const props = defineProps<{
  modelValue: boolean;
  subnetId: number;
  subnetCidr: string;
  /** 網段地址族：v6 為登錄制（新增時輸入位址、用途固定手動）。 */
  family: AddressFamily;
  /** 目標列；已指派時為編輯模式（介面不可更換）；v6 新增為 null。 */
  entry: IpEntry | null;
}>();

const emit = defineEmits<{
  "update:modelValue": [value: boolean];
  saved: [];
}>();

const $q = useQuasar();
/** 流程依賴（真 adapter）：指派／v6 登錄／取消與通知全經本埠。 */
const deps = assignmentFlowDeps($q);

const open = computed({
  get: () => props.modelValue,
  set: value => emit("update:modelValue", value)
});

const isV6 = computed(() => props.family === "ipv6");

const address = computed(() => props.entry?.address ?? "");
const existing = computed(() => props.entry?.assignment ?? null);
const isEdit = computed(() => existing.value !== null);

/** v6 新增：位址由使用者輸入（其餘情境位址唯讀）。 */
const addressEditable = computed(() => isV6.value && props.entry === null);
const newAddress = ref("");

const dialogTitle = computed(() => {
  if (isV6.value) {
    return isEdit.value ? "編輯位址" : "新增位址";
  }
  // 未指派情境更名「指派 IP 給資產」：與列上「新增資產及指派 IP」（建新資產）區別
  return isEdit.value ? "編輯指派" : "指派 IP 給資產";
});

const submitLabel = computed(() => {
  if (isV6.value) {
    return "登錄並指派";
  }
  return isEdit.value ? "儲存" : "指派";
});

/** 資產搜尋欄的組字守衛（見 .scratch/ime-composition/spec.md）。 */
const assetSelectRef = ref<QSelect | null>(null);
useCompositionGuard(assetSelectRef);

const selectedAsset = ref<Asset | null>(null);
const assetOptions = ref<Asset[]>([]);
const searchingAssets = ref(false);

const interfaces = ref<Interface[]>([]);
const loadingInterfaces = ref(false);
const selectedInterfaceId = ref<number | null>(null);

/** 每次開啟對話框更換 key、重建新增介面表單（等價舊碼的狀態重設）。 */
const newInterfaceFormKey = ref(0);
/** 新增介面表單：更換資產時僅收合（不清欄位、不清錯誤，等價舊碼的 showNewInterface = false）。 */
const newInterfaceFormRef = ref<InstanceType<typeof NewInterfaceForm> | null>(
  null
);

const purpose = ref<IpPurpose>("static");
const hostname = ref("");

const saving = ref(false);
const cancelling = ref(false);
const errorMessage = ref("");

const interfaceOptions = computed(() =>
  interfaces.value.map(item => ({
    label: interfaceLabel(item.name, item.mac),
    value: item.id
  }))
);

/** 目前（或所選）介面的 MAC；null＝無 MAC，保留停用。 */
const selectedMac = computed(() => {
  if (isEdit.value) {
    return existing.value?.mac ?? null;
  }
  const found = interfaces.value.find(
    item => item.id === selectedInterfaceId.value
  );
  return found?.mac ?? null;
});

const purposeOptions = computed(() => purposeOptionsFor(selectedMac.value));

const existingAssetLabel = computed(() => {
  const target = existing.value;
  return target === null
    ? ""
    : assetLabel(target.asset_property_no, target.asset_description);
});

const existingInterfaceLabel = computed(() => {
  const target = existing.value;
  return target === null
    ? ""
    : interfaceLabel(target.interface_name, target.mac);
});

watch(
  () => props.modelValue,
  value => {
    if (value) {
      prepare();
    }
  }
);

watch(selectedMac, mac => {
  if (mac === null) {
    purpose.value = "static";
  }
});

function prepare() {
  errorMessage.value = "";
  // 重設新增介面表單：關窗動畫中重開時子元件不會卸載，靠 key 重建
  newInterfaceFormKey.value += 1;
  selectedAsset.value = null;
  assetOptions.value = [];
  interfaces.value = [];
  selectedInterfaceId.value = null;
  hostname.value = props.entry?.assignment?.hostname ?? "";
  purpose.value = props.entry?.purpose ?? "static";
  newAddress.value = "";

  if (!isEdit.value) {
    void searchAssets("");
  }
}

/** 資產搜尋選項／選取值的顯示標籤：財產編號(描述)；為空時僅描述（見票 15）。 */
function assetOptionLabel(asset: Asset): string {
  return assetLabel(asset.property_no, asset.description);
}

function onFilterAssets(input: string, update: (callback: () => void) => void) {
  void searchAssets(input, update);
}

async function searchAssets(
  input: string,
  update?: (callback: () => void) => void
) {
  searchingAssets.value = true;
  try {
    const keyword = input.trim();
    const page = await listAssets({
      q: keyword === "" ? undefined : keyword,
      per_page: 20
    });
    if (update === undefined) {
      assetOptions.value = page.items;
    } else {
      update(() => {
        assetOptions.value = page.items;
      });
    }
  } catch (cause) {
    errorMessage.value = `搜尋資產失敗：${messageOf(cause)}`;
  } finally {
    searchingAssets.value = false;
  }
}

async function onAssetSelected(value: Asset | null) {
  interfaces.value = [];
  selectedInterfaceId.value = null;
  // 更換資產時僅收合新增介面表單（欄位值與錯誤訊息照舊保留；
  // 等價舊碼的 showNewInterface = false）
  newInterfaceFormRef.value?.collapse();

  if (value === null) {
    return;
  }

  loadingInterfaces.value = true;
  try {
    const detail = await fetchAsset(value.id);
    interfaces.value = detail.interfaces;
    if (detail.interfaces.length === 1) {
      selectedInterfaceId.value = detail.interfaces[0]?.id ?? null;
    }
  } catch (cause) {
    errorMessage.value = `讀取介面失敗：${messageOf(cause)}`;
  } finally {
    loadingInterfaces.value = false;
  }
}

/** 新增介面成功（`NewInterfaceForm` 回報）：加入清單並自動選取。 */
function onInterfaceCreated(saved: InterfaceSaved) {
  interfaces.value = [...interfaces.value, saved];
  selectedInterfaceId.value = saved.id;
}

function confirmCancel() {
  const message = cancelAssignmentConfirmMessage(address.value);
  // 回收防呆：附上該位址的最後可見與最後 MAC（僅提醒、不阻擋；見票 08）。
  $q.dialog({
    title: "取消指派",
    message: `${message}<br>${cancelAssignmentHint({
      last_seen_at: props.entry?.last_seen_at ?? null,
      last_seen_mac: props.entry?.last_seen_mac ?? null
    })}`,
    html: true,
    cancel: true,
    persistent: true
  }).onOk(() => {
    void cancel();
  });
}

async function cancel() {
  cancelling.value = true;
  errorMessage.value = "";
  try {
    // Kea 推送結果已由流程提示（時機與次數照舊）
    const result = await cancelAssignmentFlow(
      deps,
      props.subnetId,
      address.value
    );
    if (!result.ok) {
      errorMessage.value = result.message;
      return;
    }
    $q.notify({ type: "positive", message: "已取消指派" });
    emit("saved");
    open.value = false;
  } finally {
    cancelling.value = false;
  }
}

async function submit() {
  errorMessage.value = "";

  if (isV6.value) {
    // v6 編輯模式無可變欄位（僅供檢視與取消）；Enter 送出時不做事
    if (!isEdit.value) {
      await submitRegistry();
    }
    return;
  }

  saving.value = true;
  try {
    // 保留 MAC→編輯／選介面的驗證、呼叫與 Kea 提示皆在流程內
    const result = await assignFromSubnetFlow(deps, {
      subnetId: props.subnetId,
      address: address.value,
      isEdit: isEdit.value,
      existingInterfaceId: existing.value?.interface_id ?? null,
      selectedInterfaceId: selectedInterfaceId.value,
      mac: selectedMac.value,
      purpose: purpose.value,
      hostname: hostname.value
    });
    if ("error" in result) {
      errorMessage.value = result.error;
      return;
    }
    $q.notify({
      type: "positive",
      message: isEdit.value ? "已更新指派" : "已指派"
    });
    emit("saved");
    open.value = false;
  } finally {
    saving.value = false;
  }
}

/** v6 新增登錄：輸入位址＋選介面，建立即指派（用途固定 static）。 */
async function submitRegistry() {
  saving.value = true;
  try {
    // 格式與介面驗證、warnings 提示在流程內（v6 不通知 Kea，現行語意）
    const result = await registerIpv6Flow(deps, {
      subnetId: props.subnetId,
      text: newAddress.value.trim(),
      selectedInterfaceId: selectedInterfaceId.value
    });
    if ("error" in result) {
      errorMessage.value = result.error;
      return;
    }
    $q.notify({ type: "positive", message: "已登錄並指派" });
    emit("saved");
    open.value = false;
  } finally {
    saving.value = false;
  }
}

function ipv6Rule(value: string | null) {
  const text = (value ?? "").trim();
  if (text === "") {
    return "位址為必填";
  }
  return isIpv6(text) || "IPv6 格式錯誤（例：fd00::10）";
}
</script>
