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
          <div
            v-else-if="form.observed"
            class="text-caption text-positive q-mt-xs"
          >
            本機與該網段同 L2，可觀測。
          </div>
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
import { computed, ref, watch } from "vue";

import {
  createSubnet,
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
    observed: false
  };
}

const form = ref<FormState>(emptyForm());
const pools = ref<PoolDraft[]>([]);
/** 本機是否與該網段同 L2（由詳情載入；見 ADR-0015）。 */
const local = ref(false);
const saving = ref(false);
const loading = ref(false);
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
    observed: props.subnet.observed
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
      observed: detail.observed
    };
    local.value = detail.local;
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

  return {
    cidr: form.value.cidr.trim(),
    name: textOrNull(form.value.name),
    note: textOrNull(form.value.note),
    gateway: textOrNull(form.value.gateway),
    kea_subnet_id: isV4 && kea !== "" ? Number(kea) : null,
    // 新增時後端一律預設關閉；v6 不得開啟（見票 01）。
    observed: isV4 && props.subnet !== null ? form.value.observed : false,
    pools: isV4
      ? pools.value.map(row => ({
          start_ip: row.start_ip.trim(),
          end_ip: row.end_ip.trim()
        }))
      : []
  };
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
