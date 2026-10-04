<template>
  <q-dialog v-model="open" persistent>
    <q-card
      style="width: 640px; max-width: 95vw; max-height: 90vh; overflow: auto"
    >
      <q-form @submit="submit">
        <q-card-section>
          <div class="text-h6">
            {{ asset === null ? "新增資產" : "編輯資產" }}
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

          <q-input v-model="form.property_no" outlined dense label="財產編號" />

          <q-input
            v-model="form.description"
            outlined
            dense
            label="描述 *"
            :rules="[requiredRule('描述')]"
            lazy-rules
          />

          <div ref="locationField">
            <q-input
              v-model="form.location"
              outlined
              dense
              label="位置 *"
              :rules="[requiredRule('位置')]"
              lazy-rules
              @focus="locationMenuOpen = true"
              @update:model-value="locationMenuOpen = true"
            />
            <q-menu
              v-if="locationTarget !== undefined"
              v-model="locationMenuOpen"
              :target="locationTarget"
              no-parent-event
              no-focus
              no-refocus
              fit
              anchor="bottom left"
              self="top left"
            >
              <q-list dense>
                <q-item
                  v-for="suggestion in locationSuggestions"
                  :key="suggestion"
                  v-close-popup
                  clickable
                  @mousedown.prevent
                  @click="pickLocation(suggestion)"
                >
                  <q-item-section>{{ suggestion }}</q-item-section>
                </q-item>
                <q-item v-if="locationSuggestions.length === 0" disable>
                  <q-item-section class="text-grey-6">
                    沒有符合的既有位置
                  </q-item-section>
                </q-item>
              </q-list>
            </q-menu>
          </div>

          <q-input
            v-model="form.device_serial"
            outlined
            dense
            label="設備序號"
            hint="可搜尋；重複僅提示，不阻擋儲存"
          />

          <q-banner
            v-if="duplicateSerial"
            dense
            rounded
            class="bg-warning text-black"
          >
            已有其他資產使用此設備序號；仍可儲存。
          </q-banner>

          <div class="row q-col-gutter-sm">
            <div class="col-12 col-sm-6">
              <q-input v-model="form.brand" outlined dense label="廠牌" />
            </div>
            <div class="col-12 col-sm-6">
              <q-input v-model="form.model" outlined dense label="型號" />
            </div>
          </div>

          <div class="row q-col-gutter-sm">
            <div class="col-12 col-sm-6">
              <q-input
                v-model="form.purchase_date"
                outlined
                dense
                type="date"
                label="購置日期"
              />
            </div>
            <div class="col-12 col-sm-6">
              <q-input
                v-model="form.lifespan_years"
                outlined
                dense
                type="number"
                min="0"
                label="年限（年）"
                :rules="[lifespanRule]"
                lazy-rules
              />
            </div>
          </div>

          <q-input
            v-model="form.note"
            outlined
            dense
            type="textarea"
            autogrow
            label="備註"
          />
        </q-card-section>

        <q-card-section class="q-pt-none">
          <q-separator class="q-mb-md" />

          <div class="row items-center q-mb-sm">
            <div class="text-subtitle2">網路介面</div>
            <q-space />
            <q-btn
              dense
              flat
              color="primary"
              icon="add"
              label="新增介面"
              :disable="loadingInterfaces"
              @click="addInterface"
            />
          </div>

          <PeerMacHint class="q-mb-sm" @fill="fillInterfaceMac" />

          <q-banner
            v-if="interfaceError !== ''"
            dense
            rounded
            class="bg-negative text-white q-mb-sm"
          >
            {{ interfaceError }}
          </q-banner>

          <div v-if="loadingInterfaces" class="row justify-center q-pa-md">
            <q-spinner color="primary" />
          </div>

          <div
            v-else-if="interfaceDrafts.length === 0"
            class="text-grey-6 q-mb-sm"
          >
            尚無介面。MAC 空白的介面代表手動設定，須填名稱。
          </div>

          <template v-else>
            <q-card
              v-for="row in interfaceDrafts"
              :key="row.key"
              flat
              bordered
              class="q-pa-sm q-mb-sm"
            >
              <div class="row q-col-gutter-sm items-start">
                <div class="col-12 col-sm-4">
                  <q-input
                    v-model="row.name"
                    outlined
                    dense
                    label="名稱"
                    :rules="[interfaceNameRule(row)]"
                    lazy-rules
                  />
                </div>
                <div class="col-12 col-sm-4">
                  <q-input
                    v-model="row.mac"
                    outlined
                    dense
                    label="MAC"
                    :rules="[macRule]"
                    lazy-rules
                    hint="冒號／連字號／無分隔皆可"
                  />
                </div>
                <div class="col-12 col-sm-3">
                  <q-input v-model="row.note" outlined dense label="備註" />
                </div>
                <div class="col-12 col-sm-1 text-right">
                  <q-btn
                    flat
                    dense
                    round
                    icon="delete"
                    color="negative"
                    aria-label="刪除介面"
                    @click="confirmRemoveInterface(row)"
                  />
                </div>
              </div>
            </q-card>
          </template>
        </q-card-section>

        <q-card-section v-if="asset !== null" class="q-pt-none">
          <q-separator class="q-mb-md" />

          <div class="text-subtitle2 q-mb-sm">已指派 IP（唯讀）</div>

          <div v-if="loadingInterfaces" class="row justify-center q-pa-md">
            <q-spinner color="primary" />
          </div>

          <div v-else-if="assignments.length === 0" class="text-grey-6">
            尚無指派。
          </div>

          <q-list v-else dense separator>
            <q-item v-for="item in assignments" :key="item.id">
              <q-item-section>
                <q-item-label class="text-mono">{{
                  item.address
                }}</q-item-label>
                <q-item-label caption>
                  {{ item.subnet_name ? `${item.subnet_name}｜` : ""
                  }}{{ item.subnet_cidr }} ｜
                  {{ assignmentPurposeLabel(item.purpose) }}
                  <template v-if="item.hostname">
                    ｜ {{ item.hostname }}
                  </template>
                </q-item-label>
              </q-item-section>
              <q-item-section side>
                <q-btn
                  flat
                  dense
                  size="sm"
                  color="primary"
                  :to="`/subnets/${item.subnet_id}/ips`"
                  label="IP 管理"
                  @click="open = false"
                />
              </q-item-section>
            </q-item>
          </q-list>

          <div class="text-caption text-grey-7 q-mt-sm">
            指派操作一律在 IP 管理頁進行；IP 值不可修改。
          </div>
        </q-card-section>

        <q-card-actions align="right">
          <q-btn v-close-popup flat label="取消" />
          <q-btn color="primary" type="submit" label="儲存" :loading="saving" />
        </q-card-actions>
      </q-form>
    </q-card>
  </q-dialog>
</template>

<script setup lang="ts">
import { useQuasar } from "quasar";
import { computed, ref, watch } from "vue";

import {
  createAsset,
  fetchAsset,
  fetchLocations,
  findByDeviceSerial,
  updateAsset,
  type Asset,
  type AssetAssignment,
  type AssetInput
} from "@/api/assets";
import {
  createInterface,
  deleteInterface,
  updateInterface,
  type Interface,
  type InterfaceInput
} from "@/api/interfaces";
import type { IpPurpose } from "@/api/ips";
import PeerMacHint from "@/components/PeerMacHint.vue";

const props = defineProps<{
  modelValue: boolean;
  /** `null`＝新增；否則為編輯中的資產。 */
  asset: Asset | null;
}>();

const emit = defineEmits<{
  "update:modelValue": [value: boolean];
  saved: [];
}>();

const $q = useQuasar();

interface FormState {
  property_no: string;
  description: string;
  location: string;
  device_serial: string;
  brand: string;
  model: string;
  purchase_date: string;
  lifespan_years: string;
  note: string;
}

/** 對話框中的介面編輯列；`id === null`＝尚未儲存的新介面。 */
interface InterfaceDraft {
  key: number;
  id: number | null;
  name: string;
  mac: string;
  note: string;
  /** 載入／上次儲存後的原始值；新介面為 `null`。供儲存時差異比對。 */
  original: {
    name: string | null;
    mac: string | null;
    note: string | null;
  } | null;
}

function emptyForm(): FormState {
  return {
    property_no: "",
    description: "",
    location: "",
    device_serial: "",
    brand: "",
    model: "",
    purchase_date: "",
    lifespan_years: "",
    note: ""
  };
}

const form = ref<FormState>(emptyForm());
const saving = ref(false);
const errorMessage = ref("");
const allLocations = ref<string[]>([]);
const locationMenuOpen = ref(false);
const duplicateSerial = ref(false);
const locationField = ref<HTMLElement | null>(null);

const interfaceDrafts = ref<InterfaceDraft[]>([]);
/** 對話框開啟時載入的既有介面，供儲存時找出已刪除者。 */
const originalInterfaces = ref<Interface[]>([]);
/** 該資產已指派的 IP（唯讀顯示；指派一律在 IP 管理頁操作）。 */
const assignments = ref<AssetAssignment[]>([]);
const loadingInterfaces = ref(false);
const interfaceError = ref("");
/** 新增模式已建立、但介面尚未同步完成時記下的資產 id。 */
const savedAssetId = ref<number | null>(null);

/** QMenu 的定位目標；template ref 於掛載後才有值。 */
const locationTarget = computed(() => locationField.value ?? undefined);

/** 供 serial 重複檢查丟棄過期回應。 */
let serialCheckToken = 0;
/** 供介面載入丟棄過期回應。 */
let interfaceLoadToken = 0;
/** 介面編輯列的穩定 key。 */
let draftKey = 0;

const open = computed({
  get: () => props.modelValue,
  set: value => emit("update:modelValue", value)
});

const locationSuggestions = computed(() => {
  const needle = form.value.location.trim().toLowerCase();
  return allLocations.value.filter(location => {
    const candidate = location.toLowerCase();
    return (
      candidate !== needle && (needle === "" || candidate.includes(needle))
    );
  });
});

watch(
  () => props.modelValue,
  value => {
    if (value) {
      prepare();
    }
  }
);

watch(
  () => form.value.device_serial,
  serial => {
    const value = serial.trim();
    const token = ++serialCheckToken;
    duplicateSerial.value = false;

    if (value === "") {
      return;
    }

    window.setTimeout(() => {
      void checkDuplicateSerial(value, token);
    }, 400);
  }
);

function prepare() {
  const asset = props.asset;
  form.value =
    asset === null
      ? emptyForm()
      : {
          property_no: asset.property_no ?? "",
          description: asset.description,
          location: asset.location,
          device_serial: asset.device_serial ?? "",
          brand: asset.brand ?? "",
          model: asset.model ?? "",
          purchase_date: asset.purchase_date ?? "",
          lifespan_years:
            asset.lifespan_years === null ? "" : String(asset.lifespan_years),
          note: asset.note ?? ""
        };

  errorMessage.value = "";
  duplicateSerial.value = false;
  serialCheckToken += 1;
  savedAssetId.value = null;
  void loadLocations();
  void loadInterfaces();
}

async function loadLocations() {
  try {
    allLocations.value = await fetchLocations();
  } catch {
    // 建議值載入失敗不影響輸入與儲存
  }
}

/** 編輯模式載入資產詳情中的介面；新增模式無既有介面。 */
async function loadInterfaces() {
  const token = ++interfaceLoadToken;
  const asset = props.asset;

  originalInterfaces.value = [];
  interfaceDrafts.value = [];
  assignments.value = [];
  interfaceError.value = "";
  loadingInterfaces.value = false;

  if (asset === null) {
    return;
  }

  loadingInterfaces.value = true;
  try {
    const detail = await fetchAsset(asset.id);
    if (token !== interfaceLoadToken) {
      return;
    }
    originalInterfaces.value = detail.interfaces;
    interfaceDrafts.value = detail.interfaces.map(toDraft);
    assignments.value = detail.assignments;
  } catch (cause) {
    if (token === interfaceLoadToken) {
      interfaceError.value = `讀取介面失敗：${messageOf(cause)}`;
    }
  } finally {
    if (token === interfaceLoadToken) {
      loadingInterfaces.value = false;
    }
  }
}

function toDraft(item: Interface): InterfaceDraft {
  return {
    key: ++draftKey,
    id: item.id,
    name: item.name ?? "",
    mac: item.mac ?? "",
    note: item.note ?? "",
    original: { name: item.name, mac: item.mac, note: item.note }
  };
}

function addInterface() {
  interfaceDrafts.value.push({
    key: ++draftKey,
    id: null,
    name: "",
    mac: "",
    note: "",
    original: null
  });
}

/** 將連線主機 MAC 填入最後一筆 MAC 空白的介面草稿；無空白草稿時提示先新增。 */
function fillInterfaceMac(mac: string) {
  const blank = [...interfaceDrafts.value]
    .reverse()
    .find(row => row.mac.trim() === "");
  if (blank === undefined) {
    $q.notify({ type: "info", message: "請先新增介面" });
    return;
  }
  blank.mac = mac;
}

function confirmRemoveInterface(row: InterfaceDraft) {
  const label =
    row.name.trim() !== ""
      ? row.name.trim()
      : row.mac.trim() !== ""
        ? row.mac.trim()
        : "未命名介面";

  // 連動影響：已儲存介面可能已有指派（含保留；見 spec §2.2、票 08）。
  const affected =
    row.id === null
      ? 0
      : assignments.value.filter(item => item.interface_id === row.id).length;
  const impact = affected > 0 ? `將連動刪除 ${affected} 筆指派。` : "";

  $q.dialog({
    title: "刪除介面",
    message: `確定要刪除介面「${label}」？${impact}${
      row.id === null ? "（尚未儲存）" : "儲存後才會生效。"
    }`,
    cancel: true,
    persistent: true
  }).onOk(() => {
    interfaceDrafts.value = interfaceDrafts.value.filter(
      item => item.key !== row.key
    );
  });
}

async function checkDuplicateSerial(value: string, token: number) {
  try {
    const matches = await findByDeviceSerial(value);
    if (token === serialCheckToken) {
      duplicateSerial.value = matches.some(
        match => match.id !== props.asset?.id
      );
    }
  } catch {
    // 重複提示僅為輔助，查詢失敗不影響儲存
  }
}

function pickLocation(location: string) {
  form.value.location = location;
  locationMenuOpen.value = false;
}

function requiredRule(label: string) {
  return (value: string | null) =>
    (value ?? "").trim() !== "" || `${label}為必填`;
}

function lifespanRule(value: string | null) {
  const text = (value ?? "").trim();
  if (text === "") {
    return true;
  }

  const years = Number(text);
  return (Number.isInteger(years) && years >= 0) || "年限須為非負整數";
}

/** MAC 空白時名稱必填（見 spec §3.1）；有 MAC 時名稱選填。 */
function interfaceNameRule(row: InterfaceDraft) {
  return (value: string | null) => {
    if ((value ?? "").trim() !== "") {
      return true;
    }
    return row.mac.trim() !== "" || "MAC 空白時名稱為必填";
  };
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

/** 指派用途標籤（唯讀顯示）。 */
function assignmentPurposeLabel(purpose: IpPurpose): string {
  return purpose === "reservation" ? "保留" : "手動設定";
}

function messageOf(cause: unknown): string {
  return cause instanceof Error ? cause.message : String(cause);
}

function toInput(): AssetInput {
  const lifespan = form.value.lifespan_years.trim();
  return {
    property_no: textOrNull(form.value.property_no),
    description: form.value.description.trim(),
    location: form.value.location.trim(),
    device_serial: textOrNull(form.value.device_serial),
    brand: textOrNull(form.value.brand),
    model: textOrNull(form.value.model),
    purchase_date: textOrNull(form.value.purchase_date),
    lifespan_years: lifespan === "" ? null : Number(lifespan),
    note: textOrNull(form.value.note)
  };
}

function toInterfaceInput(row: InterfaceDraft): InterfaceInput {
  return {
    name: textOrNull(row.name),
    mac: textOrNull(row.mac),
    note: textOrNull(row.note)
  };
}

/** 與上次儲存值比較（MAC 以正規化後比對），避免無謂的 PATCH。 */
function interfaceChanged(row: InterfaceDraft): boolean {
  const original = row.original;
  if (original === null) {
    return true;
  }
  return (
    row.name.trim() !== (original.name ?? "") ||
    (normalizeMac(row.mac) ?? "") !== (original.mac ?? "") ||
    row.note.trim() !== (original.note ?? "")
  );
}

/**
 * 儲存介面：新增／編輯後再刪除已移除者。
 * 回傳不阻擋的警示訊息（例如全系統重複 MAC），供儲存後提示。
 */
async function syncInterfaces(assetId: number): Promise<string[]> {
  const warnings: string[] = [];
  const keptIds = new Set<number>();

  for (const row of interfaceDrafts.value) {
    const input = toInterfaceInput(row);

    if (row.id === null) {
      const saved = await createInterface(assetId, input);
      row.id = saved.id;
      row.name = saved.name ?? "";
      row.mac = saved.mac ?? "";
      row.note = saved.note ?? "";
      row.original = { name: saved.name, mac: saved.mac, note: saved.note };
      warnings.push(...saved.warnings.map(warning => warning.message));
      continue;
    }

    keptIds.add(row.id);
    if (!interfaceChanged(row)) {
      continue;
    }

    const saved = await updateInterface(row.id, input);
    row.name = saved.name ?? "";
    row.mac = saved.mac ?? "";
    row.note = saved.note ?? "";
    row.original = { name: saved.name, mac: saved.mac, note: saved.note };
    warnings.push(...saved.warnings.map(warning => warning.message));
  }

  // 已移除的既有介面：在新增／編輯成功後才刪除
  const removed: number[] = [];
  for (const item of originalInterfaces.value) {
    if (keptIds.has(item.id)) {
      continue;
    }
    await deleteInterface(item.id);
    removed.push(item.id);
  }
  originalInterfaces.value = originalInterfaces.value.filter(
    item => !removed.includes(item.id)
  );

  return warnings;
}

async function submit() {
  saving.value = true;
  errorMessage.value = "";

  try {
    const input = toInput();
    let assetId = savedAssetId.value ?? props.asset?.id ?? null;

    if (assetId === null) {
      const created = await createAsset(input);
      assetId = created.id;
      savedAssetId.value = created.id;
    } else {
      await updateAsset(assetId, input);
    }

    // 資產已寫入：立即通知外部刷新，即使後續介面同步失敗或使用者取消也不失真
    emit("saved");

    const warnings = await syncInterfaces(assetId);
    for (const message of warnings) {
      $q.notify({ type: "warning", message, timeout: 6000 });
    }

    open.value = false;
  } catch (cause) {
    errorMessage.value = messageOf(cause);
  } finally {
    saving.value = false;
  }
}
</script>
