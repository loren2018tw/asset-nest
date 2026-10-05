<template>
  <q-page padding>
    <div class="row items-center q-mb-md">
      <q-btn
        flat
        dense
        round
        icon="arrow_back"
        aria-label="返回網段列表"
        to="/ips"
      />
      <div class="text-h6 q-ml-sm">
        IP 管理
        <span v-if="subnet" class="text-grey-7 text-subtitle1 q-ml-sm">
          {{ subnet.name ? `${subnet.name}｜` : "" }}{{ subnet.cidr }}
        </span>
      </div>
    </div>

    <div class="row q-col-gutter-sm q-mb-md">
      <div class="col-12 col-md-6">
        <q-input
          v-model="filters.q"
          outlined
          dense
          clearable
          debounce="300"
          placeholder="搜尋 IP／資產描述／位置／MAC／介面名稱"
          @update:model-value="reload"
        >
          <template #prepend>
            <q-icon name="search" />
          </template>
        </q-input>
      </div>
      <div class="col-12 col-sm-6 col-md-3">
        <q-select
          v-model="filters.status"
          :options="statusOptions"
          outlined
          dense
          clearable
          emit-value
          map-options
          label="狀態／用途"
          @update:model-value="reload"
        />
      </div>
      <!-- 立即快速掃描（見票 02）；v6 無觀測，v4 未開觀測／非同 L2 時停用。
           探索掃描開啟時另提供「探索掃描」（見票 05）。 -->
      <div v-if="!isV6" class="col-12 col-sm-6 col-md-3 text-right">
        <span>
          <q-btn
            color="primary"
            outline
            icon="radar"
            label="立即掃描"
            :loading="sweeping"
            :disable="sweepDisabledReason !== null"
            @click="runQuickSweep"
          />
          <q-tooltip v-if="sweepDisabledReason">{{
            sweepDisabledReason
          }}</q-tooltip>
        </span>
        <span v-if="subnet?.discovery_enabled" class="q-ml-sm">
          <q-btn
            color="primary"
            outline
            icon="travel_explore"
            label="探索掃描"
            :loading="discovering"
            :disable="sweepDisabledReason !== null"
            @click="runDiscoverySweep"
          />
          <q-tooltip v-if="sweepDisabledReason">{{
            sweepDisabledReason
          }}</q-tooltip>
        </span>
      </div>
      <!-- v6 登錄制：僅已指派位址存在，由「新增位址」輸入並即指派（見票 06） -->
      <div v-if="isV6" class="col-12 col-sm-6 col-md-3 text-right">
        <q-btn
          color="primary"
          icon="add"
          label="新增位址"
          @click="openRegistryCreate"
        />
      </div>
    </div>

    <q-table
      :rows="ips"
      :columns="columns"
      row-key="address"
      :loading="loading"
      v-model:pagination="pagination"
      :rows-per-page-options="[10, 25, 50, 100]"
      binary-state-sort
      @request="onRequest"
    >
      <template #body-cell-address="props">
        <q-td :props="props" class="ip-address">{{ props.value }}</q-td>
      </template>
      <template #body-cell-status="props">
        <q-td :props="props">
          <q-badge :color="statusColor(props.row.status)">
            {{ statusLabel(props.row.status) }}
          </q-badge>
        </q-td>
      </template>
      <template #body-cell-location="props">
        <q-td :props="props">
          <template v-if="props.row.assignment">
            {{ props.row.assignment.asset_location }}
          </template>
          <span v-else class="text-grey-6">—</span>
        </q-td>
      </template>
      <template #body-cell-assignment="props">
        <q-td :props="props">
          <template v-if="props.row.assignment">
            <div>{{ assignmentTargetLabel(props.row.assignment) }}</div>
            <div class="text-caption text-grey-7">
              {{
                interfaceLabel(
                  props.row.assignment.interface_name,
                  props.row.assignment.mac
                )
              }}
              <template v-if="props.row.assignment.hostname">
                ｜ {{ props.row.assignment.hostname }}
              </template>
            </div>
          </template>
          <span v-else class="text-grey-6">—</span>
        </q-td>
      </template>
      <!-- 最後可見（見票 02）：未觀測／從未上線／相對時間三態 -->
      <template #body-cell-last_seen="props">
        <q-td :props="props">
          <template v-if="!props.row.observed">
            <span class="text-grey-6">未觀測</span>
            <q-tooltip>{{ unobservedHint }}</q-tooltip>
          </template>
          <template v-else-if="!props.row.last_seen_at">
            <span class="text-grey-7">從未上線</span>
            <q-tooltip>
              已開啟觀測並持續檢查；尚未看到此位址
              <template v-if="props.row.last_checked_at">
                <br />最後檢查：{{ checkedLabel(props.row.last_checked_at) }}
              </template>
            </q-tooltip>
          </template>
          <template v-else>
            <span>{{ relativeTime(props.row.last_seen_at) }}</span>
            <q-tooltip>
              來源：{{ sourceLabel(props.row.last_seen_source) }}<br />
              最後檢查：{{ checkedLabel(props.row.last_checked_at) }}
            </q-tooltip>
          </template>
        </q-td>
      </template>
      <template #body-cell-conflicts="props">
        <q-td :props="props">
          <template v-if="props.row.conflicts.length > 0">
            <q-badge
              v-for="code in props.row.conflicts"
              :key="code"
              color="warning"
              text-color="black"
              class="q-mr-xs"
            >
              {{ conflictLabel(code) }}
              <q-tooltip>{{ conflictHint(code) }}</q-tooltip>
            </q-badge>
          </template>
          <span v-else class="text-grey-6">—</span>
        </q-td>
      </template>
      <template #body-cell-actions="props">
        <q-td :props="props" class="text-right">
          <!-- 觀測歷史（見票 06）：開啟此位址的事件時間軸與用過的 MAC -->
          <q-btn
            flat
            dense
            round
            icon="history"
            aria-label="觀測歷史"
            @click="openObservations(props.row)"
          >
            <q-tooltip>觀測歷史</q-tooltip>
          </q-btn>
          <!-- 池內且未指派：不可指派（見 spec §7） -->
          <q-btn
            v-if="props.row.in_pool && !props.row.assignment"
            flat
            dense
            round
            icon="edit"
            disable
            aria-label="指派"
          >
            <q-tooltip>池內位址不可指派</q-tooltip>
          </q-btn>
          <template v-else>
            <q-btn
              flat
              dense
              round
              icon="edit"
              :aria-label="props.row.assignment ? '編輯指派' : '指派'"
              @click="openAssignment(props.row)"
            />
            <q-btn
              v-if="props.row.assignment"
              flat
              dense
              round
              icon="link_off"
              color="negative"
              aria-label="取消指派"
              @click="confirmCancel(props.row)"
            />
          </template>
        </q-td>
      </template>
      <template #no-data>
        <div class="full-width row flex-center text-grey-7 q-py-md">
          {{
            isV6
              ? "尚未登錄任何位址；請點「新增位址」建立第一筆。"
              : "沒有符合條件的位址。"
          }}
        </div>
      </template>
    </q-table>

    <assignment-dialog
      v-model="assignmentOpen"
      :subnet-id="subnetId"
      :subnet-cidr="subnet?.cidr ?? ''"
      :family="family"
      :entry="assignmentEntry"
      @saved="onAssignmentSaved"
    />

    <observation-history-dialog
      v-model="observationOpen"
      :subnet-id="subnetId"
      :address="observationEntry?.address"
    />
  </q-page>
</template>

<script setup lang="ts">
import type { QTableProps } from "quasar";
import { useQuasar } from "quasar";
import { computed, onMounted, ref } from "vue";
import { useRoute } from "vue-router";

import {
  cancelAssignment,
  listSubnetIps,
  quickSweep,
  type IpAssignmentTarget,
  type IpEntry,
  type IpSeenSource,
  type IpSortField,
  type IpStatus
} from "@/api/ips";
import {
  fetchSubnet,
  discoverySweep,
  type AddressFamily,
  type Subnet
} from "@/api/subnets";
import AssignmentDialog from "@/components/AssignmentDialog.vue";
import ObservationHistoryDialog from "@/components/ObservationHistoryDialog.vue";
import { notifyKeaSync } from "@/utils/keaSync";
import { relativeTime } from "@/utils/relativeTime";

const $q = useQuasar();
const route = useRoute();

const subnetId = Number(route.params.id);
const subnet = ref<Subnet | null>(null);
const ips = ref<IpEntry[]>([]);
const loading = ref(false);
const filters = ref<{ q: string | null; status: IpStatus | null }>({
  q: "",
  status: null
});

/**
 * 伺服器端分頁與排序；預設 IP 數值升冪（見 spec §4.3、票 14）。
 * 須以 `v-model:pagination` 綁定（而非只傳 `:pagination`），Quasar 才會把
 * 父層更新併回表格內部狀態，否則排序箭頭永遠停在初始欄位（見票 19）。
 * `binary-state-sort`：同欄點擊只在 asc／desc 間切換，不停在「取消排序」。
 */
const pagination = ref<{
  page: number;
  rowsPerPage: number;
  rowsNumber: number;
  sortBy: IpSortField;
  descending: boolean;
}>({
  page: 1,
  rowsPerPage: 50,
  rowsNumber: 0,
  sortBy: "address",
  descending: false
});

const assignmentOpen = ref(false);
const assignmentEntry = ref<IpEntry | null>(null);
/** 觀測歷史對話框（見票 06）：以列位址開啟。 */
const observationOpen = ref(false);
const observationEntry = ref<IpEntry | null>(null);
/** 「立即掃描」進行中（見票 02）。 */
const sweeping = ref(false);
/** 「探索掃描」進行中（見票 05）。 */
const discovering = ref(false);

/** v6 為登錄制（見票 06）：僅列登錄位址、用途固定 static、無 pool。 */
const isV6 = computed(() => subnet.value?.cidr.includes(":") ?? false);

/** 立即掃描停用原因（null＝可掃描；見票 02 前提）。 */
const sweepDisabledReason = computed<string | null>(() => {
  if (!subnet.value) {
    return "網段載入中";
  }
  if (!subnet.value.observed) {
    return "此網段未開啟觀測";
  }
  if (!subnet.value.local) {
    return "本機與此網段非同 L2，無法觀測";
  }
  return null;
});

/** 「未觀測」列的提示（三態定義見 spec §Further Notes）。 */
const unobservedHint = computed(() => {
  if (isV6.value) {
    return "IPv6 網段不支援觀測";
  }
  if (!subnet.value?.observed) {
    return "此網段未開啟觀測";
  }
  if (!subnet.value.local) {
    return "本機與此網段非同 L2，無法觀測";
  }
  return "位址未被已啟用的觀測涵蓋";
});

const family = computed<AddressFamily>(() => (isV6.value ? "ipv6" : "ipv4"));

/** v6 狀態恆為手動設定；其餘狀態（可用／池內／保留）不存在。 */
const statusOptions = computed<{ label: string; value: IpStatus }[]>(() =>
  isV6.value
    ? [{ label: "手動設定", value: "static" }]
    : [
        { label: "可用", value: "available" },
        { label: "池內", value: "in_pool" },
        { label: "手動設定", value: "static" },
        { label: "保留", value: "reservation" }
      ]
);

const columns: QTableProps["columns"] = [
  {
    name: "address",
    label: "IP",
    field: "address",
    align: "left",
    sortable: true
  },
  {
    name: "status",
    label: "狀態／用途",
    field: "status",
    align: "left",
    sortable: true
  },
  {
    name: "location",
    label: "位置",
    field: (row: IpEntry) => row.assignment?.asset_location ?? "",
    align: "left",
    sortable: true
  },
  {
    name: "assignment",
    label: "指派對象",
    field: "address",
    align: "left",
    sortable: true
  },
  {
    name: "last_seen",
    label: "最後可見",
    field: (row: IpEntry) => row.last_seen_at ?? "",
    align: "left",
    sortable: true
  },
  // 衝突、操作不可排序（見票 14）
  { name: "conflicts", label: "衝突", field: "conflicts", align: "left" },
  { name: "actions", label: "操作", field: "address", align: "right" }
];

/** 衝突規則代碼的中文標籤與說明（僅提示、不阻擋，見 ADR-0006）。 */
const conflictInfo: Record<string, { label: string; hint: string }> = {
  IpInPool: {
    label: "池內",
    hint: "指派的位址落在 DHCP 位址池內（僅提示，不阻擋）"
  },
  IpOutOfSubnet: {
    label: "出界",
    hint: "指派的位址不在網段 CIDR 內（僅提示，不阻擋）"
  },
  DuplicateHwAddress: {
    label: "MAC 重複",
    hint: "同一 MAC 在同一網段出現多筆保留（僅提示，不阻擋）"
  }
};

function conflictLabel(code: string): string {
  return conflictInfo[code]?.label ?? code;
}

function conflictHint(code: string): string {
  return conflictInfo[code]?.hint ?? "語意衝突（僅提示，不阻擋）";
}

function messageOf(cause: unknown): string {
  return cause instanceof Error ? cause.message : String(cause);
}

function statusLabel(status: IpStatus): string {
  switch (status) {
    case "available":
      return "可用";
    case "in_pool":
      return "池內";
    case "static":
      return "手動設定";
    case "reservation":
      return "保留";
  }
}

function statusColor(status: IpStatus): string {
  switch (status) {
    case "available":
      return "positive";
    case "in_pool":
      return "blue-grey";
    case "static":
      return "primary";
    case "reservation":
      return "purple";
  }
}

function interfaceLabel(name: string | null, mac: string | null): string {
  const label = name ?? "未命名";
  return mac === null ? `${label}（無 MAC）` : `${label} ｜ ${mac}`;
}

/** 指派對象第一行：「描述(廠牌 型號)」；缺者省略、皆缺僅描述（見 spec §4.3、票 16）。 */
function assignmentTargetLabel(target: IpAssignmentTarget): string {
  const spec = [target.asset_brand, target.asset_model]
    .map(value => value?.trim() ?? "")
    .filter(value => value !== "")
    .join(" ");
  return spec === ""
    ? target.asset_description
    : `${target.asset_description}(${spec})`;
}

/** 觀測來源標籤（見票 02；未知或缺少顯示「—」）。 */
function sourceLabel(source: IpSeenSource | null): string {
  switch (source) {
    case "arp":
      return "ARP";
    case "kea_lease":
      return "Kea 租約";
    default:
      return "—";
  }
}

/** 最後檢查時間：顯示瀏覽器本地時間；缺值為「尚未檢查」。 */
function checkedLabel(value: string | null): string {
  if (value === null) {
    return "尚未檢查";
  }
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? value : date.toLocaleString();
}

async function fetchIps() {
  loading.value = true;
  try {
    const page = await listSubnetIps(subnetId, {
      q: filters.value.q?.trim() || undefined,
      status: filters.value.status ?? undefined,
      sort: pagination.value.sortBy,
      dir: pagination.value.descending ? "desc" : "asc",
      page: pagination.value.page,
      per_page: pagination.value.rowsPerPage
    });

    ips.value = page.items;
    pagination.value.rowsNumber = page.total;
  } catch (cause) {
    $q.notify({ type: "negative", message: messageOf(cause) });
  } finally {
    loading.value = false;
  }
}

function reload() {
  pagination.value.page = 1;
  void fetchIps();
}

interface TableRequest {
  pagination: {
    page: number;
    rowsPerPage: number;
    sortBy: string | null;
    descending: boolean;
  };
}

/** q-table 的排序欄位名即欄位 `name`；僅白名單欄位可送後端（見票 14）。 */
function toSortField(value: string | null): IpSortField {
  switch (value) {
    case "address":
    case "status":
    case "location":
    case "assignment":
    case "last_seen":
      return value;
    default:
      return "address";
  }
}

function onRequest(request: TableRequest) {
  const { page, rowsPerPage, sortBy, descending } = request.pagination;
  const sort = toSortField(sortBy);
  const sortChanged =
    sort !== pagination.value.sortBy ||
    descending !== pagination.value.descending;

  pagination.value.sortBy = sort;
  pagination.value.descending = descending;
  // 切換排序時回到第 1 頁（見票 14）
  pagination.value.page = sortChanged ? 1 : page;
  pagination.value.rowsPerPage = rowsPerPage;
  void fetchIps();
}

function openAssignment(entry: IpEntry) {
  assignmentEntry.value = entry;
  assignmentOpen.value = true;
}

/** 開啟此列的觀測歷史（現況、事件時間軸、用過的 MAC；見票 06）。 */
function openObservations(entry: IpEntry) {
  observationEntry.value = entry;
  observationOpen.value = true;
}

/** v6 新增位址：無既有列，由對話框輸入位址並即指派。 */
function openRegistryCreate() {
  assignmentEntry.value = null;
  assignmentOpen.value = true;
}

function onAssignmentSaved() {
  void fetchIps();
}

/** 立即快速掃描：同步執行、回報結果後重載清單（見票 02）。 */
async function runQuickSweep() {
  sweeping.value = true;
  try {
    const report = await quickSweep(subnetId);
    $q.notify({
      type: "positive",
      message: `快速掃描完成：${report.seen}/${report.targets} 個位址有回應（${report.duration_ms} ms）`
    });
    await fetchIps();
  } catch (cause) {
    $q.notify({ type: "negative", message: messageOf(cause) });
  } finally {
    sweeping.value = false;
  }
}

/** 立即探索掃描：整段限速探測、回報結果後重載清單（見票 05）。 */
async function runDiscoverySweep() {
  discovering.value = true;
  try {
    const report = await discoverySweep(subnetId);
    $q.notify({
      type: "positive",
      message: `探索掃描完成：${report.seen}/${report.targets} 個位址有回應（${report.duration_ms} ms）`
    });
    if (subnet.value !== null && report.last_discovery_at !== undefined) {
      subnet.value = {
        ...subnet.value,
        last_discovery_at: report.last_discovery_at
      };
    }
    await fetchIps();
  } catch (cause) {
    $q.notify({ type: "negative", message: messageOf(cause) });
  } finally {
    discovering.value = false;
  }
}

function confirmCancel(entry: IpEntry) {
  const message = isV6.value
    ? `確定要取消 ${entry.address} 的指派？取消後該位址將自登錄清單移除。`
    : `確定要取消 ${entry.address} 的指派？取消後該位址回到「可用」。`;
  $q.dialog({
    title: "取消指派",
    message,
    cancel: true,
    persistent: true
  }).onOk(() => {
    void cancel(entry);
  });
}

async function cancel(entry: IpEntry) {
  try {
    const result = await cancelAssignment(subnetId, entry.address);
    notifyKeaSync($q, result.kea_sync);
    $q.notify({ type: "positive", message: "已取消指派" });
    await fetchIps();
  } catch (cause) {
    $q.notify({ type: "negative", message: messageOf(cause) });
  }
}

onMounted(async () => {
  try {
    subnet.value = await fetchSubnet(subnetId);
  } catch (cause) {
    $q.notify({ type: "negative", message: messageOf(cause) });
    return;
  }

  void fetchIps();
});
</script>

<style scoped>
.ip-address {
  font-family: monospace;
}
</style>
