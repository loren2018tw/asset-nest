<template>
  <q-dialog v-model="open" persistent>
    <q-card
      style="width: 560px; max-width: 95vw; max-height: 90vh; overflow: auto"
    >
      <q-form @submit="submit">
        <q-card-section>
          <div class="text-h6">指派 IP</div>
          <div class="text-subtitle2 text-grey-7">{{ subtitle }}</div>
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

          <!-- 位址欄：輸入前綴即時查詢候選，仍可自由輸入（如移轉既有指派） -->
          <q-select
            ref="addressSelectRef"
            v-model="address"
            :options="candidates"
            option-label="address"
            option-value="address"
            map-options
            emit-value
            use-input
            hide-selected
            fill-input
            input-debounce="300"
            outlined
            dense
            label="IP 位址 *"
            hint="IPv4／IPv6 皆可；位址須落在既有網段內"
            :rules="[addressRule]"
            lazy-rules
            @filter="onFilterCandidates"
            @input="onAddressInput"
          >
            <template #option="scope">
              <q-item v-bind="scope.itemProps">
                <q-item-section>
                  <q-item-label class="text-mono">{{
                    scope.opt.address
                  }}</q-item-label>
                  <q-item-label caption>{{
                    candidateSubnetLabel(scope.opt)
                  }}</q-item-label>
                </q-item-section>
              </q-item>
            </template>
            <template #no-option>
              <q-item v-if="candidatesSearched">
                <q-item-section class="text-grey-6">
                  此範圍無可用位址
                </q-item-section>
              </q-item>
            </template>
          </q-select>
          <div
            v-if="queryStatusHint !== null"
            class="text-caption"
            :class="queryStatusHint.color"
          >
            {{ queryStatusHint.message }}
          </div>

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

            <new-interface-form
              :key="newInterfaceFormKey"
              :asset-id="props.asset?.id ?? null"
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

        <!-- 已有指派：列於表單下方，可逐筆取消（見票 20） -->
        <q-card-section v-if="assignments.length > 0" class="q-pt-none">
          <q-separator class="q-mb-md" />

          <div class="text-subtitle2 q-mb-sm">已指派 IP</div>

          <q-list dense separator>
            <q-item v-for="item in assignments" :key="item.id">
              <q-item-section>
                <q-item-label class="text-mono">{{
                  item.address
                }}</q-item-label>
                <q-item-label caption>
                  {{ item.subnet_name ? `${item.subnet_name}｜` : ""
                  }}{{ item.subnet_cidr }} ｜
                  {{ assignmentPurposeLabel(item.purpose) }} ｜
                  {{ interfaceLabel(item.interface_name, item.mac) }}
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
                  color="negative"
                  label="取消指派"
                  :loading="cancellingId === item.id"
                  :disable="cancellingId !== null && cancellingId !== item.id"
                  @click="confirmCancelAssignment(item)"
                />
              </q-item-section>
            </q-item>
          </q-list>
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
import { useQuasar, type QSelect } from "quasar";
import { computed, ref, watch } from "vue";

import { fetchAsset, type Asset, type AssetAssignment } from "@/api/assets";
import type { AssetAssignmentSaved } from "@/api/assignments";
import type { Interface, InterfaceSaved } from "@/api/interfaces";
import {
  findIpCandidates,
  type IpCandidate,
  type IpQueryStatus
} from "@/api/ipCandidates";
import type { IpPurpose } from "@/api/ips";
import type { SubnetSummary } from "@/api/subnets";
import NewInterfaceForm from "@/components/NewInterfaceForm.vue";
import { useCompositionGuard } from "@/composables/useCompositionGuard";
import type { AssignmentDraftInput } from "@/utils/assignmentDraft";
import {
  assignFromAssetFlow,
  assignmentPurposeLabel,
  cancelAssignmentConfirmMessage,
  cancelAssignmentFlow,
  draftAssignmentFlow,
  interfaceLabel,
  purposeOptionsFor,
  statusBlockMessage
} from "@/utils/assignmentFlow";
import { assignmentFlowDeps } from "@/utils/assignmentFlowDeps";
import { assetLabel } from "@/utils/assetLabel";
import {
  parseAddress,
  parseIpv4Prefix,
  type ParsedIpv4Prefix
} from "@/utils/cidr";
import { messageOf } from "@/utils/errors";
import { normalizeMac } from "@/utils/mac";

const props = defineProps<{
  modelValue: boolean;
  /** live 模式的目標資產；草稿模式為 null（見 ADR-0025）。 */
  asset?: Asset | null;
  /** 由介面列進入時固定的目標介面；存在時隱藏介面選擇。 */
  fixedInterface?: Interface | null;
  /** 草稿模式：未儲存資產／介面的指派草稿（見 spec §4.2）；null＝live 模式。 */
  draft?: {
    /** 副標：目前表單的「財產編號(描述)（位置）（未儲存）」。 */
    subtitle: string;
    /** 草稿列目前的名稱／MAC（原文）。 */
    interfaceName: string;
    interfaceMac: string;
    /** 該介面既有（已落地＋草稿）位址；供同網段重複比對。 */
    existing: { address: string; subnetId: number | null }[];
    /** 全部網段（v6 預檢）；null＝未載入，略過預檢由後端把關。 */
    subnets: SubnetSummary[] | null;
  } | null;
}>();

const emit = defineEmits<{
  "update:modelValue": [value: boolean];
  saved: [];
  /** 草稿模式送出：回傳指派草稿（不落地）。 */
  draft: [input: AssignmentDraftInput];
}>();

const $q = useQuasar();
/** 流程依賴（真 adapter）：指派／草稿／取消與通知全經本埠。 */
const deps = assignmentFlowDeps($q);

const open = computed({
  get: () => props.modelValue,
  set: value => emit("update:modelValue", value)
});

/** 位址候選欄的組字守衛（見 .scratch/ime-composition/spec.md）。 */
const addressSelectRef = ref<QSelect | null>(null);
useCompositionGuard(addressSelectRef);

const address = ref("");
/** 位址欄候選（`@filter` 查詢結果；上限 20 筆）。 */
const candidates = ref<IpCandidate[]>([]);
/** 最近一次有效前綴查詢已完成（含 0 筆）；供「此範圍無可用位址」顯示。 */
const candidatesSearched = ref(false);
/** 完整 v4 位址的後端查詢狀態；`available` 不顯示。 */
const queryStatus = ref<IpQueryStatus | null>(null);
/** `queryStatus` 對應的查詢字串；與目前輸入不符即不顯示（兼防過期回應）。 */
const queryStatusFor = ref("");
/** 供候選查詢丟棄過期回應。 */
let candidatesToken = 0;
const interfaces = ref<Interface[]>([]);
const loadingInterfaces = ref(false);
const selectedInterfaceId = ref<number | null>(null);
/** 該資產目前已指派 IP（表單下方清單；可逐筆取消，見票 20）。 */
const assignments = ref<AssetAssignment[]>([]);
/** 取消中的指派 id；供逐列 loading 與避免重複點擊。 */
const cancellingId = ref<number | null>(null);
/** 每次開啟對話框更換 key、重建新增介面表單（等價舊碼的狀態重設）。 */
const newInterfaceFormKey = ref(0);

const purpose = ref<IpPurpose>("static");
const hostname = ref("");

const saving = ref(false);
const errorMessage = ref("");

/** 供介面清單載入丟棄過期回應。 */
let loadToken = 0;

/** 草稿模式：不做 API 指派，只回傳指派草稿（見 ADR-0025）。 */
const isDraft = computed(
  () => props.draft !== null && props.draft !== undefined
);

/** 對話框副標：live＝資產顯示；草稿＝目前表單值（含「未儲存」）。 */
const subtitle = computed(() => {
  if (isDraft.value) {
    return props.draft?.subtitle ?? "";
  }
  const asset = props.asset;
  if (asset === null || asset === undefined) {
    return "";
  }
  return `${assetLabel(asset.property_no, asset.description)}（${asset.location}）`;
});

const isFixed = computed(
  () =>
    isDraft.value ||
    (props.fixedInterface !== null && props.fixedInterface !== undefined)
);

/** 固定介面：優先取載入後的最新資料，載入完成前先用傳入值。 */
const fixedInterfaceTarget = computed<Interface | null>(() => {
  if (isDraft.value || !isFixed.value) {
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

/** 目前所選介面的 MAC（草稿模式取草稿列輸入值）；null＝無 MAC，保留停用。 */
const selectedMac = computed(() => {
  if (isDraft.value) {
    return normalizeMac(props.draft?.interfaceMac ?? "");
  }
  return selectedInterface.value?.mac ?? null;
});

const fixedInterfaceLabel = computed(() => {
  if (isDraft.value) {
    const draft = props.draft;
    if (draft === null || draft === undefined) {
      return "";
    }
    const name = draft.interfaceName.trim();
    return interfaceLabel(
      name === "" ? null : name,
      normalizeMac(draft.interfaceMac)
    );
  }
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

const purposeOptions = computed(() => purposeOptionsFor(selectedMac.value));

/**
 * `query_status` 提示（見 spec §10.3）：`available` 不顯示；與目前輸入
 * 不符（使用者已續打）即先隱藏，待新查詢結果補上。
 */
const queryStatusHint = computed<{ message: string; color: string } | null>(
  () => {
    const status = queryStatus.value;
    if (status === null || queryStatusFor.value !== address.value.trim()) {
      return null;
    }

    const blockMessage = statusBlockMessage(status.status);
    if (blockMessage !== null) {
      return { message: blockMessage, color: "text-negative" };
    }

    switch (status.status) {
      case "static":
      case "reservation":
        return {
          message: "此位址已指派；送出後可確認移轉",
          color: "text-grey-7"
        };
      default:
        return null;
    }
  }
);

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

// 輸入不再是可查詢前綴（含 v6）時立即清空候選與提示；有效前綴交由 @filter 查詢。
watch(address, value => {
  if (parseIpv4Prefix(value) === null) {
    clearCandidates();
  }
});

async function prepare() {
  const token = ++loadToken;
  errorMessage.value = "";
  // 重設新增介面表單：關窗動畫中重開時子元件不會卸載，靠 key 重建
  newInterfaceFormKey.value += 1;
  address.value = "";
  clearCandidates();
  hostname.value = "";
  purpose.value = "static";
  interfaces.value = [];
  selectedInterfaceId.value = null;
  assignments.value = [];
  cancellingId.value = null;

  const asset = props.asset;
  if (isDraft.value || asset === null || asset === undefined) {
    loadingInterfaces.value = false;
    return;
  }

  loadingInterfaces.value = true;
  try {
    const detail = await fetchAsset(asset.id);
    if (token !== loadToken) {
      return;
    }
    interfaces.value = detail.interfaces;
    assignments.value = detail.assignments;
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

/** 新增介面成功（`NewInterfaceForm` 回報）：加入清單並自動選取。 */
function onInterfaceCreated(saved: InterfaceSaved) {
  interfaces.value = [...interfaces.value, saved];
  selectedInterfaceId.value = saved.id;
}

/**
 * 自由輸入同步：q-select 的 `input-debounce` 只延後 `filter` 與
 * `update:input-value`，未選候選、未按 Enter 直接送出時仍須取得目前文字。
 * `input` 非 QSelect 宣告的事件，由 fallthrough 監聽控制項容器、隨原生
 * 事件冒泡觸發（含行動版 dialog 控制項）。
 */
function onAddressInput(event: Event) {
  const target = event.target;
  if (target instanceof HTMLInputElement) {
    address.value = target.value;
  }
}

/** 清空候選與查詢狀態，並丟棄進行中的過期回應。 */
function clearCandidates() {
  candidatesToken += 1;
  candidates.value = [];
  candidatesSearched.value = false;
  queryStatus.value = null;
  queryStatusFor.value = "";
}

/**
 * `@filter`：輸入達 2 個完整 v4 octet 才查候選（見 spec §8、§10.3）；
 * 請求失敗與過期回應皆靜默，不阻擋輸入與送出。
 */
function onFilterCandidates(
  input: string,
  update: (callback: () => void) => void,
  abort: () => void
) {
  const parsed = parseIpv4Prefix(input);
  if (parsed === null) {
    clearCandidates();
    abort();
    return;
  }

  const token = ++candidatesToken;
  void loadCandidates(parsed, token, update, abort);
}

async function loadCandidates(
  parsed: ParsedIpv4Prefix,
  token: number,
  update: (callback: () => void) => void,
  abort: () => void
) {
  try {
    const result = await findIpCandidates(parsed.query);
    if (token !== candidatesToken) {
      return;
    }
    update(() => {
      candidates.value = result.items;
      candidatesSearched.value = true;
      queryStatus.value = result.query_status;
      queryStatusFor.value = parsed.query;
    });
  } catch {
    if (token !== candidatesToken) {
      return;
    }
    clearCandidates();
    abort();
  }
}

/** 候選次要文字：網段名｜CIDR；未命名網段僅顯示 CIDR。 */
function candidateSubnetLabel(candidate: IpCandidate): string {
  return candidate.subnet_name === null
    ? candidate.subnet_cidr
    : `${candidate.subnet_name}｜${candidate.subnet_cidr}`;
}

async function submit() {
  errorMessage.value = "";

  const text = address.value.trim();

  if (isDraft.value) {
    await submitDraft(text);
    return;
  }

  const asset = props.asset;
  if (asset === null || asset === undefined) {
    return;
  }

  saving.value = true;
  try {
    // 格式→介面→保留 MAC 的驗證、呼叫順序與移轉確認皆在流程內
    const result = await assignFromAssetFlow(deps, asset.id, {
      text,
      interfaceId: selectedInterface.value?.id ?? null,
      purpose: purpose.value,
      hostname: hostname.value,
      mac: selectedMac.value
    });
    if (result.status === "error") {
      errorMessage.value = result.message;
      return;
    }
    if (result.status === "cancelled") {
      // 使用者在移轉確認中取消：不指派、不提示錯誤
      return;
    }
    onSaved(result.saved);
  } finally {
    saving.value = false;
  }
}

/**
 * 草稿模式送出（見 spec §4.3）：預檢（保留 MAC→狀態查詢→同網段衝突）後
 * 回傳指派草稿（不落地）；`statusLookup` 用本對話框的同字串快取查詢。
 */
async function submitDraft(text: string) {
  saving.value = true;
  try {
    const result = await draftAssignmentFlow({
      text,
      purpose: purpose.value,
      hostname: hostname.value,
      mac: selectedMac.value,
      existing: props.draft?.existing ?? [],
      subnets: props.draft?.subnets ?? null,
      statusLookup: statusForAddress
    });
    if ("error" in result) {
      errorMessage.value = result.error;
      return;
    }
    emit("draft", result.draft);
    open.value = false;
  } finally {
    saving.value = false;
  }
}

/** 位址查詢狀態：已有同字串結果直接用，否則即時補查；失敗回 `null`。 */
async function statusForAddress(text: string): Promise<IpQueryStatus | null> {
  if (queryStatus.value !== null && queryStatusFor.value === text) {
    return queryStatus.value;
  }
  try {
    const result = await findIpCandidates(text);
    return result.query_status;
  } catch {
    return null;
  }
}

function onSaved(saved: AssetAssignmentSaved) {
  // warnings 與 Kea 推送結果已由流程提示（時機與次數照舊）
  $q.notify({
    type: "positive",
    message: saved.transferred ? "已移轉並指派" : "已指派"
  });
  emit("saved");
  open.value = false;
}

/** 重載該資產的已指派 IP，供取消後即時更新清單。 */
async function refreshAssignments() {
  const asset = props.asset;
  if (asset === null || asset === undefined) {
    return;
  }
  const detail = await fetchAsset(asset.id);
  assignments.value = detail.assignments;
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
  cancellingId.value = item.id;
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
    await refreshAssignments();
  } catch (cause) {
    // 清單重載失敗（流程錯誤已於上方呈現）
    $q.notify({ type: "negative", message: messageOf(cause) });
  } finally {
    cancellingId.value = null;
  }
}

function addressRule(value: string | null) {
  const text = (value ?? "").trim();
  if (text === "") {
    return "位址為必填";
  }
  return parseAddress(text) !== null || "位址格式錯誤（例：10.0.0.5、fd00::5）";
}
</script>
