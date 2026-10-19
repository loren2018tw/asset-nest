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

          <!-- 編輯模式顯示觀測「最後可見」（見票 08）；無命中現況為「—」。 -->
          <div v-if="asset !== null" class="text-caption text-grey-7 q-mb-sm">
            最後可見：{{ lastSeenAt === null ? "—" : relativeTime(lastSeenAt) }}
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
                    :rules="[interfaceNameRule(row.mac)]"
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
                  <!-- 介面 MAC 觀測歷史（見票 08）；MAC 空白／無效時不顯示 -->
                  <q-btn
                    v-if="macHistoryMac(row) !== null"
                    flat
                    dense
                    round
                    icon="history"
                    aria-label="MAC 觀測歷史"
                    @click="openMacHistory(row)"
                  >
                    <q-tooltip>觀測歷史</q-tooltip>
                  </q-btn>
                  <!-- 指派：已儲存介面＝立即指派；草稿介面＝指派草稿（見 spec §4.1） -->
                  <q-btn
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

        <q-card-section
          v-if="
            asset !== null ||
            assignments.length > 0 ||
            assignmentDrafts.length > 0
          "
          class="q-pt-none"
        >
          <q-separator class="q-mb-md" />

          <div class="text-subtitle2 q-mb-sm">已指派 IP</div>

          <div v-if="loadingInterfaces" class="row justify-center q-pa-md">
            <q-spinner color="primary" />
          </div>

          <div
            v-else-if="
              assignments.length === 0 && assignmentDrafts.length === 0
            "
            class="text-grey-6"
          >
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

            <!-- 指派草稿：尚未落地，無「IP 管理」連結、取消不跳確認（見 spec §4.4） -->
            <q-item v-for="draft in assignmentDrafts" :key="draft.key">
              <q-item-section>
                <q-item-label class="text-mono">{{
                  draft.address
                }}</q-item-label>
                <q-item-label caption>
                  {{ draft.subnetLabel ? `${draft.subnetLabel} ｜ ` : ""
                  }}{{ assignmentPurposeLabel(draft.purpose) }}
                  <template v-if="draft.hostname">
                    ｜ {{ draft.hostname }}
                  </template>
                  ｜（未儲存）
                </q-item-label>
              </q-item-section>
              <q-item-section side>
                <q-btn
                  flat
                  dense
                  size="sm"
                  color="negative"
                  label="取消指派"
                  @click="removeAssignmentDraft(draft)"
                />
              </q-item-section>
            </q-item>
          </q-list>

          <div class="text-caption text-grey-7 q-mt-sm">
            指派由「指派 IP」入口進行；IP 值不可修改。
          </div>
        </q-card-section>

        <q-card-actions class="q-pa-md">
          <!-- 報廢（僅編輯模式；出借中停用＋tooltip；見 spec §5） -->
          <span v-if="asset !== null">
            <q-btn
              flat
              color="negative"
              label="報廢"
              :disable="isLentAsset"
              @click="openDisposal"
            />
            <q-tooltip v-if="isLentAsset">出借中，請先歸還</q-tooltip>
          </span>
          <q-space />
          <q-btn flat label="取消" :disable="saving" @click="requestCancel" />
          <q-btn color="primary" type="submit" label="儲存" :loading="saving" />
        </q-card-actions>
      </q-form>
    </q-card>
  </q-dialog>

  <!-- 介面列「指派 IP」：以該介面為固定目標（見票 10）；草稿列＝草稿模式（見 spec §4.2） -->
  <assign-ip-dialog
    v-if="assignInterface !== null || draftAssignContext !== null"
    v-model="assignOpen"
    :asset="asset"
    :fixed-interface="assignInterface"
    :draft="draftAssignContext"
    @saved="onAssignSaved"
    @draft="onDraftAssigned"
  />

  <!-- 取消新增時已建立資產的處置（見 spec §5.3） -->
  <q-dialog v-model="cancelConfirmOpen" persistent>
    <q-card style="width: 480px; max-width: 95vw">
      <q-card-section>
        <div class="text-h6">取消新增資產</div>
        <div class="text-body2 q-mt-sm">
          此新資產已建立，但尚未完成儲存（仍有未儲存的介面或指派）。要保留還是刪除已建立的內容？
        </div>
      </q-card-section>
      <q-card-actions align="right">
        <q-btn
          flat
          label="繼續編輯"
          :disable="deletingAsset"
          @click="cancelConfirmOpen = false"
        />
        <q-btn
          flat
          color="primary"
          label="保留已建立內容"
          :disable="deletingAsset"
          @click="keepCreatedAsset"
        />
        <q-btn
          flat
          color="negative"
          label="刪除已建立內容"
          :loading="deletingAsset"
          @click="deleteCreatedAsset"
        />
      </q-card-actions>
    </q-card>
  </q-dialog>

  <!-- 報廢：影響摘要、報廢日期與後果提示（見 spec §5） -->
  <q-dialog v-model="disposalOpen" persistent>
    <q-card style="width: 480px; max-width: 95vw">
      <q-card-section>
        <div class="text-h6">報廢資產</div>
        <div class="text-body2 q-mt-sm">
          將刪除 {{ disposalImpact.interfaceCount }} 個介面、{{
            disposalImpact.assignmentCount
          }}
          筆指派（含 {{ disposalImpact.reservationCount }} 筆保留）。
        </div>
        <div class="text-body2 q-mt-xs">
          最後可見：{{ disposalImpact.lastSeen }}；介面 MAC：{{
            disposalImpact.macLabel
          }}。
        </div>
        <div class="text-body2 q-mt-sm">
          報廢後可於「報廢資產」頁復原；介面與 IP
          指派不會回復。對話框中未儲存的變更將被捨棄。
        </div>
      </q-card-section>

      <q-card-section class="q-pt-none">
        <q-input
          v-model="disposalDate"
          type="date"
          outlined
          dense
          label="報廢日期"
          :max="today"
          :rules="[requiredRule('報廢日期')]"
          lazy-rules
        />
      </q-card-section>

      <q-card-actions align="right">
        <q-btn
          flat
          label="取消"
          :disable="disposing"
          @click="disposalOpen = false"
        />
        <q-btn
          flat
          color="negative"
          label="確認報廢"
          :loading="disposing"
          @click="confirmDispose"
        />
      </q-card-actions>
    </q-card>
  </q-dialog>

  <!-- 介面 MAC 觀測歷史：以 MAC 進入（見票 06、票 08） -->
  <observation-history-dialog
    v-model="macHistoryOpen"
    :mac="macHistoryTarget ?? undefined"
  />
</template>

<script setup lang="ts">
import { useQuasar, type QSelect } from "quasar";
import { computed, nextTick, ref, watch } from "vue";

import {
  deleteAsset,
  fetchAsset,
  fetchLocations,
  fetchTags,
  findByDeviceSerial,
  type Asset,
  type AssetAssignment,
  type AssetInput
} from "@/api/assets";
import { disposeAsset } from "@/api/disposedAssets";
import type { Interface } from "@/api/interfaces";
import type { LendingBrief } from "@/api/lendings";
import { listSubnets, type SubnetSummary } from "@/api/subnets";
import AssignIpDialog from "@/components/AssignIpDialog.vue";
import ObservationHistoryDialog from "@/components/ObservationHistoryDialog.vue";
import PeerMacHint from "@/components/PeerMacHint.vue";
import {
  sameAddress,
  type AssignmentDraftInput
} from "@/utils/assignmentDraft";
import {
  assignmentPurposeLabel,
  cancelAssignmentConfirmMessage,
  cancelAssignmentFlow,
  interfaceNameRule,
  landAssignmentDrafts,
  preflightAssignmentDrafts,
  saveAssetBody,
  syncInterfaces,
  type AssignmentDraftRow,
  type InterfaceRow
} from "@/utils/assignmentFlow";
import { assignmentFlowDeps } from "@/utils/assignmentFlowDeps";
import { summarizeAssetImpact } from "@/utils/assetImpact";
import { assetLabel } from "@/utils/assetLabel";
import { messageOf } from "@/utils/errors";
import { notifyKeaSync } from "@/utils/keaSync";
import { localDateString } from "@/utils/localDate";
import { macRule, normalizeMac } from "@/utils/mac";
import { relativeTime } from "@/utils/relativeTime";
import { textOrNull } from "@/utils/text";

const props = defineProps<{
  modelValue: boolean;
  /** `null`＝新增；否則為編輯中的資產（資產清單列可能帶出借摘要，供停用報廢；見 spec §5）。 */
  asset: (Asset & { lending?: LendingBrief | null }) | null;
  /** 新增模式的預填描述（如由 Kea 租約進入）；未提供為空字串。 */
  prefillDescription?: string;
  /** 新增模式的預填介面（如由 Kea 租約進入）；未提供不建立介面草稿。 */
  prefillInterface?: { name: string; mac: string } | null;
  /** 新增模式的預填指派草稿（如由 IP 清單進入；見 ADR-0025、
   *  `.scratch/ip-list-new-asset/spec.md`）；未提供不建立指派草稿。
   *  草稿須綁在介面草稿列上，無預填介面時自動補一筆空 MAC 的 `eth0`。 */
  prefillAssignment?: AssignmentDraftInput | null;
}>();

const emit = defineEmits<{
  "update:modelValue": [value: boolean];
  saved: [];
}>();

const $q = useQuasar();
/** 指派流程 module 的依賴（真 adapter：`@/api/*` 與 `$q`；見 spec §介面）。 */
const deps = assignmentFlowDeps($q);

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

/** 對話框中的介面編輯列（module `InterfaceRow`）；`id === null`＝尚未儲存的新介面。 */
type InterfaceDraft = InterfaceRow;

/** 指派草稿（module `AssignmentDraftRow`）：綁定介面草稿列（`rowKey`），儲存時才落地（見 spec §4、ADR-0025）。 */
type AssignmentDraft = AssignmentDraftRow;

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
/** 編輯模式載入資產詳情的「最後可見」（見票 08）；新增模式為 null。 */
const lastSeenAt = ref<string | null>(null);
/** 介面列 MAC 觀測歷史：目標 MAC 與開關（見票 06、票 08）。 */
const macHistoryOpen = ref(false);
const macHistoryTarget = ref<string | null>(null);
/** 新增模式已建立、但介面尚未同步完成時記下的資產 id。 */
const savedAssetId = ref<number | null>(null);

/** 介面列「指派 IP」對話框：固定目標介面（見票 10）。 */
const assignOpen = ref(false);
const assignInterface = ref<Interface | null>(null);
/** 指派草稿清單（新增模式與編輯模式的新介面列；見 spec §4）。 */
const assignmentDrafts = ref<AssignmentDraft[]>([]);
/** 全部網段（指派草稿的 v6 預檢與顯示）；null＝未載入，略過預檢。 */
const subnetSummaries = ref<SubnetSummary[] | null>(null);
/** 草稿模式指派對話框的目標介面草稿列。 */
const assignDraftTarget = ref<InterfaceDraft | null>(null);
/** 指派草稿的穩定 key。 */
let assignmentDraftKey = 0;
/** 取消新增時「已建立資產」的處置對話框（見 spec §5.3）。 */
const cancelConfirmOpen = ref(false);
const deletingAsset = ref(false);
/** 報廢子對話框（見 spec §5）：開關、報廢日期與送出狀態。 */
const disposalOpen = ref(false);
const disposalDate = ref(localDateString());
const disposing = ref(false);
/** 報廢日期上限：本地時區今天（開啟子對話框時更新）。 */
const today = ref(localDateString());

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

/** 編輯中的資產是否出借中（`lending` 有值）；出借中不可報廢（見 spec §2 不變量、§5）。 */
const isLentAsset = computed(() => {
  const lending = props.asset?.lending;
  return lending !== null && lending !== undefined;
});

/** 報廢子對話框的影響摘要（與 `AssetsPage` 共用 `summarizeAssetImpact`；見 spec §5）。 */
const disposalImpact = computed(() =>
  summarizeAssetImpact({
    interfaces: originalInterfaces.value,
    assignments: assignments.value,
    lastSeenAt: lastSeenAt.value
  })
);

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
      ? { ...emptyForm(), description: props.prefillDescription ?? "" }
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
  assignmentDrafts.value = [];
  subnetSummaries.value = null;
  assignDraftTarget.value = null;
  assignInterface.value = null;
  cancelConfirmOpen.value = false;
  deletingAsset.value = false;
  disposalOpen.value = false;
  disposalDate.value = localDateString();
  disposing.value = false;
  void loadLocations();
  void loadTags();
  void loadSubnets();
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

/** 全部網段：供指派草稿的 v6 預檢與顯示；失敗為 `null`（略過預檢、後端把關）。 */
async function loadSubnets() {
  try {
    subnetSummaries.value = await listSubnets();
  } catch {
    subnetSummaries.value = null;
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
  lastSeenAt.value = null;
  cancellingAssignmentId.value = null;
  interfaceError.value = "";
  loadingInterfaces.value = false;

  if (asset === null) {
    const prefill = props.prefillInterface;
    const assignment = props.prefillAssignment;
    if (prefill !== undefined && prefill !== null) {
      interfaceDrafts.value.push({
        key: ++draftKey,
        id: null,
        name: prefill.name,
        mac: prefill.mac,
        note: "",
        original: null
      });
    } else if (assignment !== undefined && assignment !== null) {
      // 指派草稿須綁在介面草稿列上（ADR-0025）；無預填介面時補一筆空 MAC 的
      // eth0（MAC 空白＝手動設定介面，可編輯）。
      interfaceDrafts.value.push({
        key: ++draftKey,
        id: null,
        name: "eth0",
        mac: "",
        note: "",
        original: null
      });
    }
    const host = interfaceDrafts.value[0];
    if (assignment !== undefined && assignment !== null && host !== undefined) {
      // 新增模式此刻至多一筆介面草稿（即預填列）；指派草稿綁在其上。
      assignmentDrafts.value.push({
        key: ++assignmentDraftKey,
        rowKey: host.key,
        ...assignment
      });
    }
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
    lastSeenAt.value = detail.last_seen_at;
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
    // 清單為空時預填 eth0（見 spec §3）
    name: interfaceDrafts.value.length === 0 ? "eth0" : "",
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

/** 介面草稿的觀測歷史入口 MAC（正規化後）；無效或空白回 null（見票 08）。 */
function macHistoryMac(row: InterfaceDraft): string | null {
  return normalizeMac(row.mac);
}

/** 以該介面 MAC 開啟觀測歷史對話框（見票 06、票 08）。 */
function openMacHistory(row: InterfaceDraft) {
  const mac = normalizeMac(row.mac);
  if (mac === null) {
    return;
  }
  macHistoryTarget.value = mac;
  macHistoryOpen.value = true;
}

function confirmRemoveInterface(row: InterfaceDraft) {
  const label =
    row.name.trim() !== ""
      ? row.name.trim()
      : row.mac.trim() !== ""
        ? row.mac.trim()
        : "未命名介面";

  // 連動影響：已儲存介面可能已有指派（含保留；見 spec §2.2、票 08）；
  // 草稿列可能已有指派草稿（見 spec §4.5）。
  const affected =
    row.id === null
      ? 0
      : assignments.value.filter(item => item.interface_id === row.id).length;
  const draftCount = assignmentDrafts.value.filter(
    item => item.rowKey === row.key
  ).length;
  const impact =
    (affected > 0 ? `將連動刪除 ${affected} 筆指派。` : "") +
    (draftCount > 0 ? `將一併取消 ${draftCount} 筆尚未儲存的指派。` : "");

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
    assignmentDrafts.value = assignmentDrafts.value.filter(
      item => item.rowKey !== row.key
    );
  });
}

/** 由介面列開啟指派對話框：已儲存列＝立即指派；草稿列＝指派草稿（見 spec §4.1）。 */
function openAssign(row: InterfaceDraft) {
  if (row.id !== null) {
    const target = originalInterfaces.value.find(item => item.id === row.id);
    if (target === undefined) {
      return;
    }
    assignInterface.value = target;
    assignDraftTarget.value = null;
  } else {
    assignInterface.value = null;
    assignDraftTarget.value = row;
  }
  assignOpen.value = true;
}

/** 草稿模式指派對話框的內容（見 spec §4.2）；非草稿列為 null。 */
const draftAssignContext = computed(() => {
  const row = assignDraftTarget.value;
  if (row === null || row.id !== null) {
    return null;
  }

  const description = form.value.description.trim();
  const label =
    description === ""
      ? "新資產"
      : assetLabel(textOrNull(form.value.property_no), description);
  const location = form.value.location.trim();

  return {
    subtitle: `${label}（${location === "" ? "未填位置" : location}）（未儲存）`,
    interfaceName: row.name,
    interfaceMac: row.mac,
    existing: existingAssignmentsFor(row),
    subnets: subnetSummaries.value
  };
});

/** 該介面列既有的位址（已落地＋指派草稿）；供同網段重複比對。 */
function existingAssignmentsFor(
  row: InterfaceDraft
): { address: string; subnetId: number | null }[] {
  const items: { address: string; subnetId: number | null }[] = [];
  if (row.id !== null) {
    for (const item of assignments.value) {
      if (item.interface_id === row.id) {
        items.push({ address: item.address, subnetId: item.subnet_id });
      }
    }
  }
  for (const draft of assignmentDrafts.value) {
    if (draft.rowKey === row.key) {
      items.push({ address: draft.address, subnetId: draft.subnetId });
    }
  }
  return items;
}

/** 草稿模式指派送出：同位址＝更新該草稿，否則新增（見 spec §4.3）。 */
function onDraftAssigned(input: AssignmentDraftInput) {
  const row = assignDraftTarget.value;
  if (row === null) {
    return;
  }

  const existing = assignmentDrafts.value.find(
    draft =>
      draft.rowKey === row.key && sameAddress(draft.address, input.address)
  );
  if (existing !== undefined) {
    existing.address = input.address;
    existing.purpose = input.purpose;
    existing.hostname = input.hostname;
    existing.subnetId = input.subnetId;
    existing.subnetLabel = input.subnetLabel;
    return;
  }

  assignmentDrafts.value.push({
    key: ++assignmentDraftKey,
    rowKey: row.key,
    ...input
  });
}

/** 移除指派草稿（尚未落地、不跳確認、不呼叫 API）。 */
function removeAssignmentDraft(draft: AssignmentDraft) {
  assignmentDrafts.value = assignmentDrafts.value.filter(
    item => item.key !== draft.key
  );
}

/** 指派對話框儲存後：更新「已指派 IP」並通知外層刷新清單；
 *  不重載介面草稿，保留未儲存編輯。 */
async function onAssignSaved() {
  emit("saved");
  const asset = props.asset;
  if (asset === null) {
    return;
  }
  await refreshAssignments(asset.id);
}

/** 目前對話框對應的資產 id：編輯模式為載入的資產；新增模式為已建立者。 */
function currentAssetId(): number | null {
  return props.asset?.id ?? savedAssetId.value;
}

/** 重載該資產的已指派 IP（取消或落地後即時反映）。 */
async function refreshAssignments(assetId: number) {
  try {
    const detail = await fetchAsset(assetId);
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
  const message = cancelAssignmentConfirmMessage(item.address);
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
  const assetId = currentAssetId();
  if (assetId === null) {
    return;
  }
  cancellingAssignmentId.value = item.id;
  try {
    const result = await cancelAssignmentFlow(
      deps,
      item.subnet_id,
      item.address
    );
    if (!result.ok) {
      $q.notify({ type: "negative", message: result.message });
      return;
    }
    $q.notify({ type: "positive", message: "已取消指派" });
    emit("saved");
    await refreshAssignments(assetId);
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

async function submit() {
  saving.value = true;
  errorMessage.value = "";

  try {
    const preflight = preflightAssignmentDrafts(
      assignmentDrafts.value,
      interfaceDrafts.value
    );
    if (preflight !== null) {
      errorMessage.value = preflight;
      return;
    }

    const body = await saveAssetBody(deps, {
      input: toInput(),
      savedAssetId: savedAssetId.value,
      editingAssetId: props.asset?.id ?? null
    });
    if (body.createdId !== null) {
      savedAssetId.value = body.createdId;
    }

    // 資產已寫入：立即通知外部刷新，即使後續介面同步失敗或使用者取消也不失真
    emit("saved");

    const sync = await syncInterfaces(deps, body.assetId, {
      rows: interfaceDrafts.value,
      original: originalInterfaces.value
    });
    interfaceDrafts.value = sync.rows;
    originalInterfaces.value = sync.original;

    // 指派草稿於介面落地後逐筆落地（見 spec §5.1）
    if (assignmentDrafts.value.length > 0) {
      const land = await landAssignmentDrafts(deps, body.assetId, {
        drafts: assignmentDrafts.value,
        rows: interfaceDrafts.value
      });
      assignmentDrafts.value = land.drafts;
      if (land.error !== null) {
        errorMessage.value = land.error;
      }
      emit("saved");
      await refreshAssignments(body.assetId);
    }

    if (assignmentDrafts.value.length === 0) {
      open.value = false;
    }
  } catch (cause) {
    errorMessage.value = messageOf(cause);
  } finally {
    saving.value = false;
  }
}

/** 取消：未曾建立資產直接關閉；已建立且尚未完成儲存時跳處置對話框（見 spec §5.3）。 */
function requestCancel() {
  if (savedAssetId.value === null) {
    open.value = false;
    return;
  }
  cancelConfirmOpen.value = true;
}

/** 保留已建立內容並關閉。 */
function keepCreatedAsset() {
  cancelConfirmOpen.value = false;
  emit("saved");
  open.value = false;
}

/** 刪除已建立內容（連動刪除介面與已落地指派）並關閉；失敗留在表單顯示錯誤。 */
async function deleteCreatedAsset() {
  const assetId = savedAssetId.value;
  if (assetId === null) {
    keepCreatedAsset();
    return;
  }

  deletingAsset.value = true;
  try {
    const result = await deleteAsset(assetId);
    notifyKeaSync(
      $q,
      result.kea_sync.find(sync => sync.status === "failed")
    );
    emit("saved");
    $q.notify({ type: "positive", message: "已刪除未完成的新資產" });
    cancelConfirmOpen.value = false;
    open.value = false;
  } catch (cause) {
    errorMessage.value = messageOf(cause);
    cancelConfirmOpen.value = false;
  } finally {
    deletingAsset.value = false;
  }
}

/** 開啟報廢子對話框：報廢日期預設今天（本地時區；見 spec §5）。 */
function openDisposal() {
  today.value = localDateString();
  disposalDate.value = today.value;
  disposalOpen.value = true;
}

/** 確認報廢：成功關閉子對話框與主對話框、通知外層；失敗留在對話框。
 *  Kea 推送失敗（`kea_sync` 任一 `failed`）另以 warning 提示一次（見 spec §5）。 */
async function confirmDispose() {
  const asset = props.asset;
  if (asset === null) {
    return;
  }

  disposing.value = true;
  try {
    const result = await disposeAsset(asset.id, {
      disposal_date: disposalDate.value
    });
    // 多筆失敗僅提示一次（訊息比照 utils/keaSync.ts）
    notifyKeaSync(
      $q,
      result.kea_sync.find(sync => sync.status === "failed")
    );
    $q.notify({ type: "positive", message: "已報廢資產" });
    disposalOpen.value = false;
    open.value = false;
    emit("saved");
  } catch (cause) {
    $q.notify({ type: "negative", message: messageOf(cause) });
  } finally {
    disposing.value = false;
  }
}
</script>
