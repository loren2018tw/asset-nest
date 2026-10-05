<template>
  <q-dialog v-model="open" persistent>
    <q-card
      style="width: 640px; max-width: 95vw; max-height: 90vh; overflow: auto"
    >
      <q-form @submit="submit">
        <q-card-section>
          <div class="text-h6">
            {{ subnet === null ? "新增網段" : "編輯網段" }}
          </div>
        </q-card-section>

        <q-linear-progress v-if="loading" indeterminate color="primary" />

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
            v-model="form.cidr"
            outlined
            dense
            label="CIDR *"
            :rules="[cidrRule]"
            lazy-rules
            :error="cidrIssue !== undefined"
            :error-message="cidrIssue?.message"
            hint="單一地址族；例：192.168.1.0/24、fd00::/64"
          />

          <q-input
            v-model="form.name"
            outlined
            dense
            label="名稱"
            hint="選填；不強制唯一（雙棧可同名）"
          />

          <q-input
            v-model="form.gateway"
            outlined
            dense
            label="gateway"
            :error="gatewayIssue !== undefined"
            :error-message="gatewayIssue?.message"
            hint="選填；須在 CIDR 內（僅標記，該位址仍可被指派）"
          />

          <q-input
            v-if="family === 'ipv4'"
            v-model="form.kea_subnet_id"
            outlined
            dense
            type="number"
            label="Kea subnet-id"
            :rules="[keaSubnetIdRule]"
            lazy-rules
            hint="選填；僅 IPv4，全系統唯一"
          />

          <q-input
            v-model="form.note"
            outlined
            dense
            type="textarea"
            autogrow
            label="備註"
          />
        </q-card-section>

        <q-card-section v-if="family === 'ipv4'" class="q-pt-none">
          <q-separator class="q-mb-md" />

          <div class="row items-center q-mb-sm">
            <div class="text-subtitle2">DHCP 位址池（pool）</div>
            <q-space />
            <q-btn
              dense
              flat
              color="primary"
              icon="add"
              label="新增 pool"
              @click="addPool"
            />
          </div>

          <div v-if="pools.length === 0" class="text-grey-6 q-mb-sm">
            尚無 pool。pool 內位址不可指派；範圍須在 CIDR 內且彼此不重疊。
          </div>

          <q-banner
            v-if="poolIssues.length > 0"
            dense
            rounded
            class="bg-negative text-white q-mb-sm"
          >
            <div v-for="issue in poolIssues" :key="issue.message">
              {{ issue.message }}
            </div>
          </q-banner>

          <q-card
            v-for="row in pools"
            :key="row.key"
            flat
            bordered
            class="q-pa-sm q-mb-sm"
          >
            <div class="row q-col-gutter-sm items-start">
              <div class="col-12 col-sm-5">
                <q-input
                  v-model="row.start_ip"
                  outlined
                  dense
                  label="起點 *"
                  :rules="[poolAddressRule('起點')]"
                  lazy-rules
                />
              </div>
              <div class="col-12 col-sm-5">
                <q-input
                  v-model="row.end_ip"
                  outlined
                  dense
                  label="終點 *"
                  :rules="[poolAddressRule('終點')]"
                  lazy-rules
                />
              </div>
              <div class="col-12 col-sm-2 text-right">
                <q-btn
                  flat
                  dense
                  round
                  icon="delete"
                  color="negative"
                  aria-label="刪除 pool"
                  @click="removePool(row)"
                />
              </div>
            </div>
          </q-card>
        </q-card-section>

        <q-card-section
          v-if="family === 'ipv4' && subnet !== null"
          class="q-pt-none"
        >
          <q-separator class="q-mb-md" />

          <div class="text-subtitle2 q-mb-sm">觀測</div>
          <q-toggle v-model="form.observed" label="開啟觀測（快速掃描）" />
          <q-banner
            v-if="form.observed && !local"
            dense
            rounded
            class="bg-warning text-black q-mt-sm"
          >
            v1 無法觀測（本機非同 L2）
          </q-banner>
          <template v-else-if="form.observed">
            <div class="text-caption text-positive q-mt-xs">
              本機與該網段同 L2，可觀測。
            </div>

            <q-separator class="q-my-md" />
            <div class="text-subtitle2 q-mb-sm"> 探索掃描（尋找未知設備） </div>
            <q-toggle v-model="form.discovery_enabled" label="開啟探索掃描" />
            <div class="text-caption text-grey-7 q-mt-xs">
              定期限速探測網段全部位址，找出未指派但有主的位址與未登錄 MAC。
            </div>

            <q-input
              v-if="form.discovery_enabled"
              v-model="form.discovery_interval"
              outlined
              dense
              type="number"
              label="探索間隔（分鐘）"
              class="q-mt-sm"
              :rules="[discoveryIntervalRule]"
              lazy-rules
              hint="留空＝使用全站預設（OBSERVATION_DISCOVERY_INTERVAL_SECS）"
            />

            <div class="row items-center q-mt-sm">
              <div class="text-caption text-grey-7">
                上次探索：{{ lastDiscoveryLabel }}
              </div>
              <q-space />
              <q-btn
                color="primary"
                outline
                dense
                icon="radar"
                label="立即快速掃描"
                class="q-mr-sm"
                :loading="quickSweeping"
                :disable="sweeping"
                @click="runQuickSweep"
              />
              <q-btn
                color="primary"
                outline
                dense
                icon="travel_explore"
                label="立即探索"
                :loading="sweeping"
                :disable="!form.discovery_enabled || quickSweeping"
                @click="runDiscovery"
              />
            </div>
            <div class="text-caption text-grey-6 q-mt-xs">
              立即探索依已儲存的設定執行；未儲存的變更請先按「儲存」。
            </div>
          </template>
          <div v-else class="text-caption text-grey-7 q-mt-xs">
            開啟後將定期探測已指派與有租約位址；v1 僅支援本機同 L2 的 IPv4
            網段。
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

import { quickSweep } from "@/api/ips";
import {
  createSubnet,
  discoverySweep,
  fetchSubnet,
  updateSubnet,
  type AddressFamily,
  type SubnetInput,
  type SubnetSummary
} from "@/api/subnets";
import {
  cidrContainsAddress,
  cidrsOverlap,
  parseAddress,
  parseCidr,
  type ParsedCidr
} from "@/utils/cidr";
import { relativeTime } from "@/utils/relativeTime";

const $q = useQuasar();

const props = defineProps<{
  modelValue: boolean;
  /** `null`＝新增；否則為編輯中的網段（列表摘要，開啟後載入完整內容）。 */
  subnet: SubnetSummary | null;
  /** 既有網段清單，供即時重疊檢查（後端仍為權威）。 */
  subnets: SubnetSummary[];
}>();

const emit = defineEmits<{
  "update:modelValue": [value: boolean];
  saved: [];
}>();

interface FormState {
  cidr: string;
  name: string;
  gateway: string;
  kea_subnet_id: string;
  note: string;
  observed: boolean;
  discovery_enabled: boolean;
  /** 探索間隔（分鐘）；空字串＝全站預設（見票 05）。 */
  discovery_interval: string;
}

/** 對話框中的 pool 編輯列。 */
interface PoolDraft {
  key: number;
  start_ip: string;
  end_ip: string;
}

function emptyForm(): FormState {
  return {
    cidr: "",
    name: "",
    gateway: "",
    kea_subnet_id: "",
    note: "",
    observed: false,
    discovery_enabled: false,
    discovery_interval: ""
  };
}

const form = ref<FormState>(emptyForm());
const pools = ref<PoolDraft[]>([]);
/** 本機是否與該網段同 L2（由詳情載入；見 ADR-0015）。 */
const local = ref(false);
/** 上次探索時間（UTC；由詳情載入，立即探索後更新；見票 05）。 */
const lastDiscoveryAt = ref<string | null>(null);
const saving = ref(false);
const loading = ref(false);
/** 「立即探索」進行中（見票 05）。 */
const sweeping = ref(false);
/** 「立即快速掃描」進行中（見票 02）。 */
const quickSweeping = ref(false);
const errorMessage = ref("");

/** 供載入詳情丟棄過期回應。 */
let loadToken = 0;
/** pool 編輯列的穩定 key。 */
let poolKey = 0;

const open = computed({
  get: () => props.modelValue,
  set: value => emit("update:modelValue", value)
});

/** 由 CIDR 輸入判斷地址族；v6 隱藏 pool 與 kea_subnet_id（見 spec §2.3）。 */
const family = computed<AddressFamily>(() =>
  form.value.cidr.includes(":") ? "ipv6" : "ipv4"
);

/** 上次探索的顯示文字（相對時間；從未探索為「尚未探索」；見票 05）。 */
const lastDiscoveryLabel = computed(() => {
  if (lastDiscoveryAt.value === null) {
    return "尚未探索";
  }
  const label = relativeTime(lastDiscoveryAt.value);
  return label === "剛看到" ? "剛剛" : label;
});

/** 表單內容的即時結構問題（見 spec §4.2）；後端仍為權威。 */
interface LiveIssue {
  field: "cidr" | "gateway" | "pools";
  message: string;
}

/** 即時結構檢查：與既有網段重疊、gateway 不在 CIDR 內、pool 範圍。 */
function collectIssues(): LiveIssue[] {
  const issues: LiveIssue[] = [];
  const cidrText = form.value.cidr.trim();
  const cidr = parseCidr(cidrText);
  if (cidr === null) {
    return issues;
  }

  for (const other of props.subnets) {
    if (other.id === props.subnet?.id) {
      continue;
    }
    const otherCidr = parseCidr(other.cidr);
    if (otherCidr !== null && cidrsOverlap(cidr, otherCidr)) {
      const label = other.name
        ? `「${other.name}」（${other.cidr}）`
        : ` ${other.cidr} `;
      issues.push({
        field: "cidr",
        message: `與既有網段${label}重疊（網段不得重疊，含嵌套）`
      });
      break;
    }
  }

  const gatewayText = form.value.gateway.trim();
  if (gatewayText !== "") {
    const gateway = parseAddress(gatewayText);
    if (gateway !== null && !cidrContainsAddress(cidr, gateway)) {
      issues.push({
        field: "gateway",
        message: `gateway ${gatewayText} 不在網段 ${cidrText} 內`
      });
    }
  }

  if (family.value === "ipv4") {
    issues.push(...collectPoolIssues(cidr, cidrText));
  }

  return issues;
}

/** pool 各段的即時檢查：僅接受 IPv4、起訖順序、範圍與段間重疊。 */
function collectPoolIssues(cidr: ParsedCidr, cidrText: string): LiveIssue[] {
  const issues: LiveIssue[] = [];
  const valid: {
    index: number;
    start: bigint;
    end: bigint;
    startText: string;
    endText: string;
  }[] = [];

  pools.value.forEach((row, index) => {
    const startText = row.start_ip.trim();
    const endText = row.end_ip.trim();
    const start = parseAddress(startText);
    const end = parseAddress(endText);
    if (start === null || end === null) {
      return; // 格式與必填由輸入規則與後端把關
    }
    if (start.family !== "ipv4" || end.family !== "ipv4") {
      issues.push({
        field: "pools",
        message: `pools[${index}] 僅接受 IPv4 位址`
      });
      return;
    }
    if (start.value > end.value) {
      issues.push({
        field: "pools",
        message: `pools[${index}] 起點 ${startText} 不可大於終點 ${endText}`
      });
      return;
    }
    if (!cidrContainsAddress(cidr, start) || !cidrContainsAddress(cidr, end)) {
      issues.push({
        field: "pools",
        message: `pools[${index}] 範圍 ${startText}–${endText} 不在網段 ${cidrText} 內`
      });
      return;
    }
    valid.push({
      index,
      start: start.value,
      end: end.value,
      startText,
      endText
    });
  });

  for (let i = 0; i < valid.length; i += 1) {
    const a = valid[i];
    if (a === undefined) {
      continue;
    }
    for (let j = i + 1; j < valid.length; j += 1) {
      const b = valid[j];
      if (b === undefined) {
        continue;
      }
      if (a.start <= b.end && b.start <= a.end) {
        issues.push({
          field: "pools",
          message: `pools[${a.index}]（${a.startText}–${a.endText}）與 pools[${b.index}]（${b.startText}–${b.endText}）重疊`
        });
      }
    }
  }

  return issues;
}

const liveIssues = computed(collectIssues);
const cidrIssue = computed(() =>
  liveIssues.value.find(issue => issue.field === "cidr")
);
const gatewayIssue = computed(() =>
  liveIssues.value.find(issue => issue.field === "gateway")
);
const poolIssues = computed(() =>
  liveIssues.value.filter(issue => issue.field === "pools")
);

// 表單內容變更後，先前的錯誤（即時或後端）可能已過時。
watch(liveIssues, () => {
  errorMessage.value = "";
});

watch(
  () => props.modelValue,
  value => {
    if (value) {
      void prepare();
    }
  }
);

async function prepare() {
  const token = ++loadToken;
  errorMessage.value = "";
  form.value = emptyForm();
  pools.value = [];
  local.value = false;
  lastDiscoveryAt.value = null;
  sweeping.value = false;
  loading.value = false;

  if (props.subnet === null) {
    return;
  }

  form.value = {
    cidr: props.subnet.cidr,
    name: props.subnet.name ?? "",
    gateway: "",
    kea_subnet_id: "",
    note: "",
    observed: props.subnet.observed,
    discovery_enabled: false,
    discovery_interval: ""
  };
  local.value = props.subnet.local;

  loading.value = true;
  try {
    const detail = await fetchSubnet(props.subnet.id);
    if (token !== loadToken) {
      return;
    }
    form.value = {
      cidr: detail.cidr,
      name: detail.name ?? "",
      gateway: detail.gateway ?? "",
      kea_subnet_id:
        detail.kea_subnet_id === null ? "" : String(detail.kea_subnet_id),
      note: detail.note ?? "",
      observed: detail.observed,
      discovery_enabled: detail.discovery_enabled,
      discovery_interval:
        detail.discovery_interval_minutes === null
          ? ""
          : String(detail.discovery_interval_minutes)
    };
    local.value = detail.local;
    lastDiscoveryAt.value = detail.last_discovery_at;
    pools.value = detail.pools.map(pool => ({
      key: ++poolKey,
      start_ip: pool.start_ip,
      end_ip: pool.end_ip
    }));
  } catch (cause) {
    if (token === loadToken) {
      errorMessage.value = `讀取網段失敗：${messageOf(cause)}`;
    }
  } finally {
    if (token === loadToken) {
      loading.value = false;
    }
  }
}

function addPool() {
  pools.value.push({ key: ++poolKey, start_ip: "", end_ip: "" });
}

function removePool(row: PoolDraft) {
  pools.value = pools.value.filter(item => item.key !== row.key);
}

function cidrRule(value: string | null) {
  const text = (value ?? "").trim();
  if (text === "") {
    return "CIDR 為必填";
  }
  return text.includes("/") || "CIDR 格式錯誤（例：192.168.1.0/24）";
}

function keaSubnetIdRule(value: string | null) {
  const text = (value ?? "").trim();
  if (text === "") {
    return true;
  }
  return Number.isInteger(Number(text)) || "Kea subnet-id 須為整數";
}

/** 探索間隔：留空＝全站預設；否則須為正整數分鐘（見票 05）。 */
function discoveryIntervalRule(value: string | null) {
  const text = (value ?? "").trim();
  if (text === "") {
    return true;
  }
  const minutes = Number(text);
  return (
    (Number.isInteger(minutes) && minutes > 0) ||
    "探索間隔須為正整數分鐘（留空＝全站預設）"
  );
}

function poolAddressRule(label: string) {
  return (value: string | null) =>
    (value ?? "").trim() !== "" || `${label}為必填`;
}

function textOrNull(value: string): string | null {
  const text = value.trim();
  return text === "" ? null : text;
}

function messageOf(cause: unknown): string {
  return cause instanceof Error ? cause.message : String(cause);
}

function toInput(): SubnetInput {
  const kea = form.value.kea_subnet_id.trim();
  const isV4 = family.value === "ipv4";
  const isExisting = props.subnet !== null;
  const observed = isV4 && isExisting ? form.value.observed : false;
  const interval = form.value.discovery_interval.trim();

  return {
    cidr: form.value.cidr.trim(),
    name: textOrNull(form.value.name),
    note: textOrNull(form.value.note),
    gateway: textOrNull(form.value.gateway),
    kea_subnet_id: isV4 && kea !== "" ? Number(kea) : null,
    // 新增時後端一律預設關閉；v6 不得開啟（見票 01）。
    observed,
    // 關閉觀測時一併關閉探索（後端要求探索需已開觀測，見票 05）。
    discovery_enabled: observed && form.value.discovery_enabled,
    discovery_interval_minutes: interval === "" ? null : Number(interval),
    pools: isV4
      ? pools.value.map(row => ({
          start_ip: row.start_ip.trim(),
          end_ip: row.end_ip.trim()
        }))
      : []
  };
}

/** 立即快速掃描：同步執行並回報結果（依已儲存的觀測設定；見票 02）。 */
async function runQuickSweep() {
  if (props.subnet === null) {
    return;
  }

  quickSweeping.value = true;
  try {
    const report = await quickSweep(props.subnet.id);
    $q.notify({
      type: "positive",
      message: `快速掃描完成：${report.seen}/${report.targets} 個位址有回應（${report.duration_ms} ms）`
    });
  } catch (cause) {
    $q.notify({ type: "negative", message: messageOf(cause) });
  } finally {
    quickSweeping.value = false;
  }
}

/** 立即探索：同步執行並更新上次探索時間（依已儲存的設定；見票 05）。 */
async function runDiscovery() {
  if (props.subnet === null) {
    return;
  }

  sweeping.value = true;
  try {
    const report = await discoverySweep(props.subnet.id);
    lastDiscoveryAt.value = report.last_discovery_at ?? lastDiscoveryAt.value;
    $q.notify({
      type: "positive",
      message: `探索掃描完成：${report.seen}/${report.targets} 個位址有回應（${report.duration_ms} ms）`
    });
  } catch (cause) {
    $q.notify({ type: "negative", message: messageOf(cause) });
  } finally {
    sweeping.value = false;
  }
}

async function submit() {
  const issue = liveIssues.value[0];
  if (issue !== undefined) {
    errorMessage.value = issue.message;
    return;
  }

  saving.value = true;
  errorMessage.value = "";

  try {
    const input = toInput();
    if (props.subnet === null) {
      await createSubnet(input);
    } else {
      await updateSubnet(props.subnet.id, input);
    }

    emit("saved");
    open.value = false;
  } catch (cause) {
    errorMessage.value = messageOf(cause);
  } finally {
    saving.value = false;
  }
}
</script>
