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

          <q-select
            ref="tagSelect"
            v-model="form.tags"
            :options="tagOptions"
            multiple
            use-input
            use-chips
            new-value-mode="add-unique"
            outlined
            dense
            label="標籤"
            hint="自由文字、可多個；輸入後按 Enter 新增"
            @filter="filterTags"
            @add="onTagAdded"
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
                <div class="col-12 col-sm-2">
                  <q-input v-model="row.note" outlined dense label="備註" />
                </div>
                <div class="col-12 col-sm-2 text-right">
                  <!-- 指派一律以已儲存的介面為對象（未儲存介面不顯示） -->
                  <q-btn
                    v-if="row.id !== null"
                    flat
                    dense
                    round
                    icon="add_link"
                    color="primary"
                    aria-label="指派 IP"
                    @click="openAssign(row)"
                  />
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

          <div class="text-subtitle2 q-mb-sm">已指派 IP</div>

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
                <div class="row q-gutter-xs">
                  <q-btn
                    flat
                    dense
                    size="sm"
                    color="primary"
                    :to="`/subnets/${item.subnet_id}/ips`"
                    label="IP 管理"
                    @click="open = false"
                  />
                  <q-btn
                    flat
                    dense
                    size="sm"
                    color="negative"
                    label="取消指派"
                    :loading="cancellingAssignmentId === item.id"
                    :disable="
                      cancellingAssignmentId !== null &&
                      cancellingAssignmentId !== item.id
                    "
                    @click="confirmCancelAssignment(item)"
                  />
                </div>
              </q-item-section>
            </q-item>
          </q-list>

          <div class="text-caption text-grey-7 q-mt-sm">
            指派由「指派 IP」入口進行；IP 值不可修改。
          </div>
        </q-card-section>

        <q-card-actions align="right">
          <q-btn v-close-popup flat label="取消" />
          <q-btn color="primary" type="submit" label="儲存" :loading="saving" />
        </q-card-actions>
      </q-form>
    </q-card>
  </q-dialog>

  <!-- 介面列「指派 IP」：以該介面為固定目標（見票 10） -->
  <assign-ip-dialog
    v-if="asset !== null && assignInterface !== null"
    v-model="assignOpen"
    :asset="asset"
    :fixed-interface="assignInterface"
    @saved="onAssignSaved"
  />
</template>

<script setup lang="ts">
import { useQuasar, type QSelect } from "quasar";
import { computed, nextTick, ref, watch } from "vue";

import {
  createAsset,
  fetchAsset,
  fetchLocations,
  fetchTags,
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
import { cancelAssignment, type IpPurpose } from "@/api/ips";
import AssignIpDialog from "@/components/AssignIpDialog.vue";
import PeerMacHint from "@/components/PeerMacHint.vue";
import { parseAddress } from "@/utils/cidr";
import { notifyKeaSync } from "@/utils/keaSync";

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
  tags: string[];
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
    note: "",
    tags: []
  };
}

const form = ref<FormState>(emptyForm());
const saving = ref(false);
const errorMessage = ref("");
const allLocations = ref<string[]>([]);
/** 既有標籤建議（後端去重）；篩選後供 q-select 顯示。 */
const allTags = ref<string[]>([]);
const tagOptions = ref<string[]>([]);
const locationMenuOpen = ref(false);
const duplicateSerial = ref(false);
const locationField = ref<HTMLElement | null>(null);
/** 標籤欄位：點選既有標籤後需以公開方法清空輸入文字（見票 22）。 */
const tagSelect = ref<QSelect | null>(null);

const interfaceDrafts = ref<InterfaceDraft[]>([]);
/** 對話框開啟時載入的既有介面，供儲存時找出已刪除者。 */
const originalInterfaces = ref<Interface[]>([]);
/** 該資產已指派的 IP（清單顯示；可逐筆取消，見票 20）。 */
const assignments = ref<AssetAssignment[]>([]);
/** 取消中的指派 id；供逐列 loading 與避免重複點擊。 */
const cancellingAssignmentId = ref<number | null>(null);
const loadingInterfaces = ref(false);
const interfaceError = ref("");
/** 新增模式已建立、但介面尚未同步完成時記下的資產 id。 */
const savedAssetId = ref<number | null>(null);

/** 介面列「指派 IP」對話框：固定目標介面（見票 10）。 */
const assignOpen = ref(false);
const assignInterface = ref<Interface | null>(null);

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
          note: asset.note ?? "",
          tags: [...asset.tags]
        };

  errorMessage.value = "";
  duplicateSerial.value = false;
  serialCheckToken += 1;
  savedAssetId.value = null;
  void loadLocations();
  void loadTags();
  void loadInterfaces();
}

async function loadLocations() {
  try {
    allLocations.value = await fetchLocations();
  } catch {
    // 建議值載入失敗不影響輸入與儲存
  }
}

async function loadTags() {
  try {
    allTags.value = await fetchTags();
    tagOptions.value = allTags.value;
  } catch {
    // 建議值載入失敗不影響輸入與儲存
  }
}

/** 依輸入過濾既有標籤，並排除已選者（新標籤仍可直接輸入新增）。
 *  須呼叫 `update()` 讓 Quasar 結束 loading 並開啟選單（否則轉圈不止、選項不顯示）。 */
function filterTags(input: string, update: (callback: () => void) => void) {
  update(() => {
    const needle = input.toLowerCase();
    const selected = new Set(form.value.tags.map(tag => tag.toLowerCase()));
    tagOptions.value = allTags.value.filter(
      tag =>
        !selected.has(tag.toLowerCase()) && tag.toLowerCase().includes(needle)
    );
  });
}

/** 點選既有標籤後清空輸入文字（Quasar 滑鼠點選路徑不會自動清空，見票 22）；
 *  待 model 更新後再重跑過濾，讓剛選取的標籤自建議清單移除。 */
function onTagAdded() {
  void nextTick(() => {
    tagSelect.value?.updateInputValue("", false);
  });
}

/** 編輯模式載入資產詳情中的介面；新增模式無既有介面。 */
async function loadInterfaces() {
  const token = ++interfaceLoadToken;
  const asset = props.asset;

  originalInterfaces.value = [];
  interfaceDrafts.value = [];
  assignments.value = [];
  cancellingAssignmentId.value = null;
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

/** 由已儲存的介面列開啟指派對話框（目標介面固定；未儲存介面不顯示按鈕）。 */
function openAssign(row: InterfaceDraft) {
  if (row.id === null) {
    return;
  }
  const target = originalInterfaces.value.find(item => item.id === row.id);
  if (target === undefined) {
    return;
  }
  assignInterface.value = target;
  assignOpen.value = true;
}

/** 指派對話框儲存後：更新「已指派 IP」並通知外層刷新清單；
 *  不重載介面草稿，保留未儲存編輯。 */
async function onAssignSaved() {
  emit("saved");
  await refreshAssignments();
}

/** 重載該資產的已指派 IP（取消後即時反映）。 */
async function refreshAssignments() {
  const asset = props.asset;
  if (asset === null) {
    return;
  }
  try {
    const detail = await fetchAsset(asset.id);
    assignments.value = detail.assignments;
  } catch (cause) {
    $q.notify({
      type: "negative",
      message: `讀取指派失敗：${messageOf(cause)}`
    });
  }
}

/** 取消指派前確認；v6 取消後自登錄清單移除（見票 06）。 */
function confirmCancelAssignment(item: AssetAssignment) {
  const message =
    parseAddress(item.address)?.family === "ipv6"
      ? `確定要取消 ${item.address} 的指派？取消後該位址將自登錄清單移除。`
      : `確定要取消 ${item.address} 的指派？取消後該位址回到「可用」。`;
  $q.dialog({
    title: "取消指派",
    message,
    cancel: true,
    persistent: true
  }).onOk(() => {
    void cancelAssignmentItem(item);
  });
}

async function cancelAssignmentItem(item: AssetAssignment) {
  cancellingAssignmentId.value = item.id;
  try {
    const result = await cancelAssignment(item.subnet_id, item.address);
    notifyKeaSync($q, result.kea_sync);
    $q.notify({ type: "positive", message: "已取消指派" });
    emit("saved");
    await refreshAssignments();
  } catch (cause) {
    $q.notify({ type: "negative", message: messageOf(cause) });
  } finally {
    cancellingAssignmentId.value = null;
  }
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
    note: textOrNull(form.value.note),
    tags: form.value.tags
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
