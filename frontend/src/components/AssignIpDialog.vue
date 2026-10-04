<template>
  <q-dialog v-model="open" persistent>
    <q-card
      style="width: 560px; max-width: 95vw; max-height: 90vh; overflow: auto"
    >
      <q-form @submit="submit">
        <q-card-section>
          <div class="text-h6">指派 IP</div>
          <div class="text-subtitle2 text-grey-7">
            {{ asset.description }}（{{ asset.location }}）
          </div>
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

          <q-input
            v-model="address"
            outlined
            dense
            label="IP 位址 *"
            hint="IPv4／IPv6 皆可；位址須落在既有網段內"
            :rules="[addressRule]"
            lazy-rules
          />

          <!-- 由介面列進入：目標介面固定；否則選介面或當場新增 -->
          <q-input
            v-if="isFixed"
            :model-value="fixedInterfaceLabel"
            outlined
            dense
            readonly
            label="介面"
          />
          <template v-else>
            <q-select
              v-model="selectedInterfaceId"
              :options="interfaceOptions"
              :loading="loadingInterfaces"
              emit-value
              map-options
              outlined
              dense
              label="介面 *"
              :hint="
                !loadingInterfaces && interfaces.length === 0
                  ? '此資產尚無介面，請新增介面'
                  : undefined
              "
            />

            <div>
              <q-btn
                dense
                flat
                color="primary"
                icon="add"
                label="新增介面"
                :disable="savingInterface"
                @click="showNewInterface = !showNewInterface"
              />
            </div>

            <q-card v-if="showNewInterface" flat bordered class="q-pa-sm">
              <q-banner
                v-if="newInterfaceError !== ''"
                dense
                rounded
                class="bg-negative text-white q-mb-sm"
              >
                {{ newInterfaceError }}
              </q-banner>
              <div class="row q-col-gutter-sm items-start">
                <div class="col-12 col-sm-5">
                  <q-input
                    v-model="newInterface.name"
                    outlined
                    dense
                    label="名稱"
                    :rules="[newInterfaceNameRule]"
                    lazy-rules
                  />
                </div>
                <div class="col-12 col-sm-5">
                  <q-input
                    v-model="newInterface.mac"
                    outlined
                    dense
                    label="MAC"
                    :rules="[macRule]"
                    lazy-rules
                  />
                </div>
                <div class="col-12 col-sm-2 text-right">
                  <q-btn
                    dense
                    flat
                    color="primary"
                    label="建立"
                    :loading="savingInterface"
                    @click="createNewInterface"
                  />
                </div>
              </div>
              <PeerMacHint class="q-mt-xs" @fill="fillNewInterfaceMac" />
            </q-card>
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
            <div v-if="selectedMac === null" class="text-caption text-grey-7">
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
          <q-btn v-close-popup flat label="取消" />
          <q-btn color="primary" type="submit" label="指派" :loading="saving" />
        </q-card-actions>
      </q-form>
    </q-card>
  </q-dialog>
</template>

<script setup lang="ts">
import { useQuasar } from "quasar";
import { computed, ref, watch } from "vue";

import { fetchAsset, type Asset } from "@/api/assets";
import { assignFromAsset, type AssetAssignmentSaved } from "@/api/assignments";
import { ApiError } from "@/api/client";
import { createInterface, type Interface } from "@/api/interfaces";
import type { IpPurpose } from "@/api/ips";
import PeerMacHint from "@/components/PeerMacHint.vue";
import { parseAddress } from "@/utils/cidr";

const props = defineProps<{
  modelValue: boolean;
  asset: Asset;
  /** 由介面列進入時固定的目標介面；存在時隱藏介面選擇。 */
  fixedInterface?: Interface | null;
}>();

const emit = defineEmits<{
  "update:modelValue": [value: boolean];
  saved: [];
}>();

const $q = useQuasar();

const open = computed({
  get: () => props.modelValue,
  set: value => emit("update:modelValue", value)
});

const address = ref("");
const interfaces = ref<Interface[]>([]);
const loadingInterfaces = ref(false);
const selectedInterfaceId = ref<number | null>(null);

const showNewInterface = ref(false);
const newInterface = ref({ name: "", mac: "" });
const savingInterface = ref(false);
const newInterfaceError = ref("");

const purpose = ref<IpPurpose>("static");
const hostname = ref("");

const saving = ref(false);
const errorMessage = ref("");

/** 供介面清單載入丟棄過期回應。 */
let loadToken = 0;

const isFixed = computed(
  () => props.fixedInterface !== null && props.fixedInterface !== undefined
);

/** 固定介面：優先取載入後的最新資料，載入完成前先用傳入值。 */
const fixedInterfaceTarget = computed<Interface | null>(() => {
  if (!isFixed.value) {
    return null;
  }
  return (
    interfaces.value.find(item => item.id === props.fixedInterface?.id) ??
    props.fixedInterface ??
    null
  );
});

const selectedInterface = computed<Interface | null>(() => {
  if (isFixed.value) {
    return fixedInterfaceTarget.value;
  }
  if (selectedInterfaceId.value === null) {
    return null;
  }
  return (
    interfaces.value.find(item => item.id === selectedInterfaceId.value) ?? null
  );
});

/** 目前所選介面的 MAC；null＝無 MAC，保留停用。 */
const selectedMac = computed(() => selectedInterface.value?.mac ?? null);

const fixedInterfaceLabel = computed(() => {
  const target = fixedInterfaceTarget.value;
  return target === null ? "" : interfaceLabel(target.name, target.mac);
});

const interfaceOptions = computed(() =>
  interfaces.value.map(item => ({
    label: interfaceLabel(item.name, item.mac),
    value: item.id
  }))
);

/** 依輸入位址判斷地址族：v6 用途固定手動、不顯示選項（見 spec §7）。 */
const isV6 = computed(() => parseAddress(address.value)?.family === "ipv6");

const purposeOptions = computed<
  { label: string; value: IpPurpose; disable: boolean }[]
>(() => [
  { label: "手動設定（static）", value: "static", disable: false },
  {
    label: "DHCPv4 保留（reservation）",
    value: "reservation",
    disable: selectedMac.value === null
  }
]);

// immediate：由父層以 v-if 掛載並直接開啟時（modelValue 初始為 true）也要載入
watch(
  () => props.modelValue,
  value => {
    if (value) {
      void prepare();
    }
  },
  { immediate: true }
);

watch(selectedMac, mac => {
  if (mac === null) {
    purpose.value = "static";
  }
});

watch(isV6, value => {
  if (value) {
    purpose.value = "static";
    hostname.value = "";
  }
});

async function prepare() {
  const token = ++loadToken;
  errorMessage.value = "";
  newInterfaceError.value = "";
  address.value = "";
  hostname.value = "";
  purpose.value = "static";
  showNewInterface.value = false;
  newInterface.value = { name: "", mac: "" };
  interfaces.value = [];
  selectedInterfaceId.value = null;

  loadingInterfaces.value = true;
  try {
    const detail = await fetchAsset(props.asset.id);
    if (token !== loadToken) {
      return;
    }
    interfaces.value = detail.interfaces;
    if (!isFixed.value && detail.interfaces.length === 1) {
      selectedInterfaceId.value = detail.interfaces[0]?.id ?? null;
    }
  } catch (cause) {
    if (token === loadToken) {
      errorMessage.value = `讀取介面失敗：${messageOf(cause)}`;
    }
  } finally {
    if (token === loadToken) {
      loadingInterfaces.value = false;
    }
  }
}

/** 介面顯示：名稱／MAC；無 MAC 時標示。 */
function interfaceLabel(name: string | null, mac: string | null): string {
  const label = name ?? "未命名";
  return mac === null ? `${label}（無 MAC）` : `${label} ｜ ${mac}`;
}

/** 將連線主機 MAC 填入當場新增介面的 MAC 欄位。 */
function fillNewInterfaceMac(mac: string) {
  newInterface.value.mac = mac;
}

/** 當場新增介面（名稱／MAC），成功後自動選取。 */
async function createNewInterface() {
  const name = newInterface.value.name.trim();
  const mac = newInterface.value.mac.trim();

  if (name === "" && mac === "") {
    newInterfaceError.value = "MAC 空白時名稱為必填";
    return;
  }
  if (mac !== "" && normalizeMac(mac) === null) {
    newInterfaceError.value =
      "MAC 格式錯誤：須為 12 位十六進位（可用冒號、連字號或無分隔）";
    return;
  }

  savingInterface.value = true;
  newInterfaceError.value = "";
  try {
    const saved = await createInterface(props.asset.id, {
      name: textOrNull(name),
      mac: textOrNull(mac),
      note: null
    });
    for (const warning of saved.warnings) {
      $q.notify({ type: "warning", message: warning.message, timeout: 6000 });
    }
    interfaces.value = [...interfaces.value, saved];
    selectedInterfaceId.value = saved.id;
    showNewInterface.value = false;
    newInterface.value = { name: "", mac: "" };
  } catch (cause) {
    newInterfaceError.value = messageOf(cause);
  } finally {
    savingInterface.value = false;
  }
}

async function submit() {
  errorMessage.value = "";

  const text = address.value.trim();
  if (parseAddress(text) === null) {
    errorMessage.value = "IP 位址格式錯誤（例：10.0.0.5、fd00::5）";
    return;
  }

  const target = selectedInterface.value;
  if (target === null) {
    errorMessage.value = "請選擇介面";
    return;
  }

  if (purpose.value === "reservation" && selectedMac.value === null) {
    errorMessage.value = "所選介面無 MAC，不可設為保留";
    return;
  }

  const input = {
    address: text,
    interface_id: target.id,
    purpose: isV6.value ? ("static" as const) : purpose.value,
    hostname:
      !isV6.value && purpose.value === "reservation"
        ? textOrNull(hostname.value)
        : null
  };

  saving.value = true;
  try {
    try {
      const saved = await assignFromAsset(props.asset.id, {
        ...input,
        transfer: false
      });
      onSaved(saved);
    } catch (cause) {
      // 已指派給其他介面：提示目前對象，確認後以 transfer=true 重試
      const elsewhere = parseAssignedElsewhere(cause);
      if (elsewhere === null) {
        throw cause;
      }

      const confirmed = await confirmTransfer(text, elsewhere);
      if (!confirmed) {
        return;
      }

      const saved = await assignFromAsset(props.asset.id, {
        ...input,
        transfer: true
      });
      onSaved(saved);
    }
  } catch (cause) {
    errorMessage.value = messageOf(cause);
  } finally {
    saving.value = false;
  }
}

function onSaved(saved: AssetAssignmentSaved) {
  for (const warning of saved.warnings) {
    $q.notify({ type: "warning", message: warning.message, timeout: 6000 });
  }
  $q.notify({
    type: "positive",
    message: saved.transferred ? "已移轉並指派" : "已指派"
  });
  emit("saved");
  open.value = false;
}

/** 後端「位址已指派給其他介面」提示所需欄位（見票 10）。 */
interface AssignedElsewhere {
  asset_description: string;
  asset_location: string;
  interface_name: string | null;
  mac: string | null;
}

/** 解析 `address_assigned_elsewhere` 錯誤；非此原因回傳 `null`。 */
function parseAssignedElsewhere(cause: unknown): AssignedElsewhere | null {
  if (
    !(cause instanceof ApiError) ||
    cause.details?.reason !== "address_assigned_elsewhere"
  ) {
    return null;
  }

  const details = cause.details;
  return {
    asset_description: textOf(details.asset_description, "未知資產"),
    asset_location: textOf(details.asset_location, "未知位置"),
    interface_name: nullableTextOf(details.interface_name),
    mac: nullableTextOf(details.mac)
  };
}

/** 移轉確認：顯示目前指派對象，確定後回傳 true。 */
function confirmTransfer(
  addressText: string,
  target: AssignedElsewhere
): Promise<boolean> {
  const interfaceName = target.interface_name ?? "未命名";
  const mac = target.mac ?? "無 MAC";

  return new Promise(resolve => {
    $q.dialog({
      title: "位址已指派",
      message:
        `位址 ${addressText} 目前已指派給「${target.asset_description}（${target.asset_location}）」` +
        `的介面「${interfaceName}（${mac}）」。確定要將此指派移轉到目前介面？`,
      cancel: true,
      persistent: true
    })
      .onOk(() => resolve(true))
      .onCancel(() => resolve(false))
      .onDismiss(() => resolve(false));
  });
}

function addressRule(value: string | null) {
  const text = (value ?? "").trim();
  if (text === "") {
    return "位址為必填";
  }
  return parseAddress(text) !== null || "位址格式錯誤（例：10.0.0.5、fd00::5）";
}

function newInterfaceNameRule(value: string | null) {
  if ((value ?? "").trim() !== "") {
    return true;
  }
  return newInterface.value.mac.trim() !== "" || "MAC 空白時名稱為必填";
}

function macRule(value: string | null) {
  const text = (value ?? "").trim();
  if (text === "") {
    return true;
  }
  return (
    normalizeMac(text) !== null ||
    "MAC 格式錯誤：須為 12 位十六進位（可用冒號、連字號或無分隔）"
  );
}

/** 將常見 MAC 輸入格式正規化為小寫冒號格式；無法解析回傳 `null`。 */
function normalizeMac(text: string): string | null {
  const hex = text.replace(/[:.-]/g, "").toLowerCase();
  if (!/^[0-9a-f]{12}$/.test(hex)) {
    return null;
  }
  return hex.match(/.{2}/g)?.join(":") ?? null;
}

function textOrNull(value: string): string | null {
  const text = value.trim();
  return text === "" ? null : text;
}

/** 讀取 details 中的字串欄位；缺漏或空白改用 fallback。 */
function textOf(value: unknown, fallback: string): string {
  return typeof value === "string" && value !== "" ? value : fallback;
}

function nullableTextOf(value: unknown): string | null {
  return typeof value === "string" && value !== "" ? value : null;
}

function messageOf(cause: unknown): string {
  return cause instanceof Error ? cause.message : String(cause);
}
</script>
