<template>
  <q-dialog v-model="open" persistent>
    <q-card
      style="width: 560px; max-width: 95vw; max-height: 90vh; overflow: auto"
    >
      <q-form @submit="submit">
        <q-card-section>
          <div class="text-h6">
            {{ isEdit ? "編輯指派" : "指派 IP" }}
          </div>
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

          <q-input
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
              v-model="selectedAsset"
              :options="assetOptions"
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
                    <q-item-label>{{ scope.opt.description }}</q-item-label>
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

            <div>
              <q-btn
                dense
                flat
                color="primary"
                icon="add"
                label="新增介面"
                :disable="selectedAsset === null || savingInterface"
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
            </q-card>
          </template>

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
          <q-btn
            color="primary"
            type="submit"
            :label="isEdit ? '儲存' : '指派'"
            :loading="saving"
          />
        </q-card-actions>
      </q-form>
    </q-card>
  </q-dialog>
</template>

<script setup lang="ts">
import { useQuasar } from "quasar";
import { computed, ref, watch } from "vue";

import { fetchAsset, listAssets, type Asset } from "@/api/assets";
import { createInterface, type Interface } from "@/api/interfaces";
import {
  assignIp,
  cancelAssignment,
  type IpEntry,
  type IpPurpose
} from "@/api/ips";

const props = defineProps<{
  modelValue: boolean;
  subnetId: number;
  subnetCidr: string;
  /** 目標列；已指派時為編輯／改用途模式（介面不可更換）。 */
  entry: IpEntry | null;
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

const address = computed(() => props.entry?.address ?? "");
const existing = computed(() => props.entry?.assignment ?? null);
const isEdit = computed(() => existing.value !== null);

const selectedAsset = ref<Asset | null>(null);
const assetOptions = ref<Asset[]>([]);
const searchingAssets = ref(false);

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

const existingAssetLabel = computed(() => {
  const target = existing.value;
  return target === null
    ? ""
    : `${target.asset_description}（${target.asset_location}）`;
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
  newInterfaceError.value = "";
  selectedAsset.value = null;
  assetOptions.value = [];
  interfaces.value = [];
  selectedInterfaceId.value = null;
  showNewInterface.value = false;
  newInterface.value = { name: "", mac: "" };
  hostname.value = props.entry?.assignment?.hostname ?? "";
  purpose.value = props.entry?.purpose ?? "static";

  if (!isEdit.value) {
    void searchAssets("");
  }
}

/** 介面顯示：名稱／MAC；無 MAC 時標示。 */
function interfaceLabel(name: string | null, mac: string | null): string {
  const label = name ?? "未命名";
  return mac === null ? `${label}（無 MAC）` : `${label} ｜ ${mac}`;
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
  showNewInterface.value = false;

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

/** 當場新增介面（名稱／MAC），成功後自動選取。 */
async function createNewInterface() {
  const asset = selectedAsset.value;
  if (asset === null) {
    return;
  }

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
    const saved = await createInterface(asset.id, {
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

function confirmCancel() {
  $q.dialog({
    title: "取消指派",
    message: `確定要取消 ${address.value} 的指派？取消後該位址回到「可用」。`,
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
    await cancelAssignment(props.subnetId, address.value);
    $q.notify({ type: "positive", message: "已取消指派" });
    emit("saved");
    open.value = false;
  } catch (cause) {
    errorMessage.value = messageOf(cause);
  } finally {
    cancelling.value = false;
  }
}

async function submit() {
  errorMessage.value = "";

  if (purpose.value === "reservation" && selectedMac.value === null) {
    errorMessage.value = "所選介面無 MAC，不可設為保留";
    return;
  }

  let interfaceId: number;
  if (isEdit.value) {
    const target = existing.value;
    if (target === null) {
      errorMessage.value = "缺少指派資料";
      return;
    }
    interfaceId = target.interface_id;
  } else {
    if (selectedInterfaceId.value === null) {
      errorMessage.value = "請選擇介面";
      return;
    }
    interfaceId = selectedInterfaceId.value;
  }

  saving.value = true;
  try {
    await assignIp(props.subnetId, address.value, {
      interface_id: interfaceId,
      purpose: purpose.value,
      hostname:
        purpose.value === "reservation" ? textOrNull(hostname.value) : null
    });
    $q.notify({
      type: "positive",
      message: isEdit.value ? "已更新指派" : "已指派"
    });
    emit("saved");
    open.value = false;
  } catch (cause) {
    errorMessage.value = messageOf(cause);
  } finally {
    saving.value = false;
  }
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

function messageOf(cause: unknown): string {
  return cause instanceof Error ? cause.message : String(cause);
}
</script>
