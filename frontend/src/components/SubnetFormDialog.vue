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
                  :rules="[addressRule('起點')]"
                  lazy-rules
                />
              </div>
              <div class="col-12 col-sm-5">
                <q-input
                  v-model="row.end_ip"
                  outlined
                  dense
                  label="終點 *"
                  :rules="[addressRule('終點')]"
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

        <q-card-section v-if="family === 'ipv4'" class="q-pt-none">
          <q-separator class="q-mb-md" />

          <div class="row items-center q-mb-sm">
            <div class="text-subtitle2">排除範圍</div>
            <q-space />
            <q-btn
              dense
              flat
              color="primary"
              icon="add"
              label="新增排除範圍"
              @click="addExclusion"
            />
          </div>

          <div v-if="exclusions.length === 0" class="text-grey-6 q-mb-sm">
            排除範圍內位址不可指派（例：NAT 對外）；不得與 pool 或彼此重疊
          </div>

          <q-banner
            v-if="exclusionIssues.length > 0"
            dense
            rounded
            class="bg-negative text-white q-mb-sm"
          >
            <div v-for="issue in exclusionIssues" :key="issue.message">
              {{ issue.message }}
            </div>
          </q-banner>

          <q-card
            v-for="row in exclusions"
            :key="row.key"
            flat
            bordered
            class="q-pa-sm q-mb-sm"
          >
            <div class="row q-col-gutter-sm items-start">
              <div class="col-12 col-sm-4">
                <q-input
                  v-model="row.start_ip"
                  outlined
                  dense
                  label="起點 *"
                  :rules="[addressRule('起點')]"
                  lazy-rules
                />
              </div>
              <div class="col-12 col-sm-4">
                <q-input
                  v-model="row.end_ip"
                  outlined
                  dense
                  label="終點 *"
                  :rules="[addressRule('終點')]"
                  lazy-rules
                />
              </div>
              <div class="col-12 col-sm-3">
                <q-input
                  v-model="row.note"
                  outlined
                  dense
                  label="用途說明"
                  hint="選填；不可含 |"
                />
              </div>
              <div class="col-12 col-sm-1 text-right">
                <q-btn
                  flat
                  dense
                  round
                  icon="delete"
                  color="negative"
                  aria-label="刪除排除範圍"
                  @click="removeExclusion(row)"
                />
              </div>
            </div>
          </q-card>
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
}

/** 對話框中的 pool 編輯列。 */
interface PoolDraft {
  key: number;
  start_ip: string;
  end_ip: string;
}

/** 對話框中的排除範圍編輯列。 */
interface ExclusionDraft {
  key: number;
  start_ip: string;
  end_ip: string;
  note: string;
}

function emptyForm(): FormState {
  return {
    cidr: "",
    name: "",
    gateway: "",
    kea_subnet_id: "",
    note: ""
  };
}

const form = ref<FormState>(emptyForm());
const pools = ref<PoolDraft[]>([]);
const exclusions = ref<ExclusionDraft[]>([]);
const saving = ref(false);
const loading = ref(false);
const errorMessage = ref("");

/** 供載入詳情丟棄過期回應。 */
let loadToken = 0;
/** pool 編輯列的穩定 key。 */
let poolKey = 0;
/** 排除範圍編輯列的穩定 key。 */
let exclusionKey = 0;

const open = computed({
  get: () => props.modelValue,
  set: value => emit("update:modelValue", value)
});

/** 由 CIDR 輸入判斷地址族；v6 隱藏 pool、排除範圍與 kea_subnet_id（見 spec §2.3）。 */
const family = computed<AddressFamily>(() =>
  form.value.cidr.includes(":") ? "ipv6" : "ipv4"
);

/** 表單內容的即時結構問題（見 spec §4.2）；後端仍為權威。 */
interface LiveIssue {
  field: "cidr" | "gateway" | "pools" | "exclusions";
  message: string;
}

/** 解析後可比較的範圍列（pool／排除範圍共用）。 */
interface ValidRange {
  index: number;
  start: bigint;
  end: bigint;
  startText: string;
  endText: string;
}

/** 即時結構檢查：與既有網段重疊、gateway 不在 CIDR 內、pool 與排除範圍。 */
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
    const poolResult = collectPoolIssues(cidr, cidrText);
    issues.push(...poolResult.issues);
    issues.push(...collectExclusionIssues(cidr, cidrText, poolResult.valid));
  }

  return issues;
}

/** pool 各段的即時檢查：僅接受 IPv4、起訖順序、範圍與段間重疊。 */
function collectPoolIssues(
  cidr: ParsedCidr,
  cidrText: string
): { issues: LiveIssue[]; valid: ValidRange[] } {
  const parsed = collectValidRanges(pools.value, "pools", cidr, cidrText);
  return {
    issues: [...parsed.issues, ...collectOverlaps(parsed.valid, "pools")],
    valid: parsed.valid
  };
}

/** 排除範圍各段的即時檢查：段內規則外，另檢彼此與 pool 重疊、用途說明分隔符。 */
function collectExclusionIssues(
  cidr: ParsedCidr,
  cidrText: string,
  validPools: ValidRange[]
): LiveIssue[] {
  const parsed = collectValidRanges(
    exclusions.value,
    "exclusions",
    cidr,
    cidrText
  );
  const issues: LiveIssue[] = [
    ...parsed.issues,
    ...collectOverlaps(parsed.valid, "exclusions")
  ];

  for (const exclusion of parsed.valid) {
    for (const pool of validPools) {
      if (exclusion.start <= pool.end && pool.start <= exclusion.end) {
        issues.push({
          field: "exclusions",
          message:
            `exclusions[${exclusion.index}]（${exclusion.startText}–${exclusion.endText}）` +
            `與 pools[${pool.index}]（${pool.startText}–${pool.endText}）重疊` +
            `（排除範圍不得與 DHCP 位址池重疊）`
        });
      }
    }
  }

  // 用途說明不得含 CSV 分隔符 `|`（trim 後判定）。
  exclusions.value.forEach((row, index) => {
    if (row.note.trim().includes("|")) {
      issues.push({
        field: "exclusions",
        message: `exclusions[${index}] 用途說明不可包含 |`
      });
    }
  });

  return issues;
}

/** pool／排除範圍共用的逐列檢查：僅接受 IPv4、起訖順序、範圍須在 CIDR 內。 */
function collectValidRanges(
  rows: { start_ip: string; end_ip: string }[],
  kind: "pools" | "exclusions",
  cidr: ParsedCidr,
  cidrText: string
): { issues: LiveIssue[]; valid: ValidRange[] } {
  const issues: LiveIssue[] = [];
  const valid: ValidRange[] = [];

  rows.forEach((row, index) => {
    const startText = row.start_ip.trim();
    const endText = row.end_ip.trim();
    const start = parseAddress(startText);
    const end = parseAddress(endText);
    if (start === null || end === null) {
      return; // 格式與必填由輸入規則與後端把關
    }
    if (start.family !== "ipv4" || end.family !== "ipv4") {
      issues.push({
        field: kind,
        message: `${kind}[${index}] 僅接受 IPv4 位址`
      });
      return;
    }
    if (start.value > end.value) {
      issues.push({
        field: kind,
        message: `${kind}[${index}] 起點 ${startText} 不可大於終點 ${endText}`
      });
      return;
    }
    if (!cidrContainsAddress(cidr, start) || !cidrContainsAddress(cidr, end)) {
      issues.push({
        field: kind,
        message: `${kind}[${index}] 範圍 ${startText}–${endText} 不在網段 ${cidrText} 內`
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

  return { issues, valid };
}

/** 同一清單內的兩兩重疊檢查（端點皆含）。 */
function collectOverlaps(
  valid: ValidRange[],
  kind: "pools" | "exclusions"
): LiveIssue[] {
  const issues: LiveIssue[] = [];

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
          field: kind,
          message: `${kind}[${a.index}]（${a.startText}–${a.endText}）與 ${kind}[${b.index}]（${b.startText}–${b.endText}）重疊`
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
const exclusionIssues = computed(() =>
  liveIssues.value.filter(issue => issue.field === "exclusions")
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
  exclusions.value = [];
  loading.value = false;

  if (props.subnet === null) {
    return;
  }

  form.value = {
    cidr: props.subnet.cidr,
    name: props.subnet.name ?? "",
    gateway: "",
    kea_subnet_id: "",
    note: ""
  };

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
      note: detail.note ?? ""
    };
    pools.value = detail.pools.map(pool => ({
      key: ++poolKey,
      start_ip: pool.start_ip,
      end_ip: pool.end_ip
    }));
    exclusions.value = detail.exclusions.map(exclusion => ({
      key: ++exclusionKey,
      start_ip: exclusion.start_ip,
      end_ip: exclusion.end_ip,
      note: exclusion.note ?? ""
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

function addExclusion() {
  exclusions.value.push({
    key: ++exclusionKey,
    start_ip: "",
    end_ip: "",
    note: ""
  });
}

function removeExclusion(row: ExclusionDraft) {
  exclusions.value = exclusions.value.filter(item => item.key !== row.key);
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

function addressRule(label: string) {
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
    pools: isV4
      ? pools.value.map(row => ({
          start_ip: row.start_ip.trim(),
          end_ip: row.end_ip.trim()
        }))
      : [],
    exclusions: isV4
      ? exclusions.value.map(row => ({
          start_ip: row.start_ip.trim(),
          end_ip: row.end_ip.trim(),
          note: textOrNull(row.note)
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
