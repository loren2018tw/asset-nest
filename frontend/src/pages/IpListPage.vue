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

    <q-banner v-if="isV6" rounded class="bg-grey-2 text-grey-8 q-mb-md">
      IPv6 網段採登錄制（僅已指派位址存在），IP 清單尚未支援（見票 06）。
    </q-banner>

    <template v-else>
      <div class="row q-col-gutter-sm q-mb-md">
        <div class="col-12 col-md-6">
          <q-input
            v-model="filters.q"
            outlined
            dense
            clearable
            debounce="300"
            placeholder="搜尋 IP／資產描述／MAC／介面名稱"
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
      </div>

      <q-table
        :rows="ips"
        :columns="columns"
        row-key="address"
        :loading="loading"
        :pagination="pagination"
        :rows-per-page-options="[10, 25, 50, 100]"
        @request="onRequest"
      >
        <template #body-cell-address="props">
          <q-td :props="props" class="ip-address">{{ props.value }}</q-td>
        </template>
        <template #body-cell-is_gateway="props">
          <q-td :props="props" class="text-center">
            <q-badge v-if="props.row.is_gateway" color="primary">
              Gateway
            </q-badge>
          </q-td>
        </template>
        <template #body-cell-status="props">
          <q-td :props="props">
            <q-badge :color="statusColor(props.row.status)">
              {{ statusLabel(props.row.status) }}
            </q-badge>
          </q-td>
        </template>
        <template #body-cell-assignment="props">
          <q-td :props="props">
            <template v-if="props.row.assignment">
              <div>
                {{ props.row.assignment.asset_description }}（{{
                  props.row.assignment.asset_location
                }}）
              </div>
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
        <template #body-cell-actions="props">
          <q-td :props="props" class="text-right">
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
      </q-table>
    </template>

    <assignment-dialog
      v-model="assignmentOpen"
      :subnet-id="subnetId"
      :subnet-cidr="subnet?.cidr ?? ''"
      :entry="assignmentEntry"
      @saved="onAssignmentSaved"
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
  type IpEntry,
  type IpStatus
} from "@/api/ips";
import { fetchSubnet, type Subnet } from "@/api/subnets";
import AssignmentDialog from "@/components/AssignmentDialog.vue";

const $q = useQuasar();
const route = useRoute();

const subnetId = Number(route.params.id);
const subnet = ref<Subnet | null>(null);
const ips = ref<IpEntry[]>([]);
const loading = ref(false);
const filters = ref<{ q: string; status: IpStatus | null }>({
  q: "",
  status: null
});
const pagination = ref({ page: 1, rowsPerPage: 50, rowsNumber: 0 });

const assignmentOpen = ref(false);
const assignmentEntry = ref<IpEntry | null>(null);

/** v6 為登錄制、尚未支援瀏覽（見票 06），不呼叫 IP API。 */
const isV6 = computed(() => subnet.value?.cidr.includes(":") ?? false);

const statusOptions: { label: string; value: IpStatus }[] = [
  { label: "可用", value: "available" },
  { label: "池內", value: "in_pool" },
  { label: "手動設定", value: "static" },
  { label: "保留", value: "reservation" }
];

const columns: QTableProps["columns"] = [
  { name: "address", label: "IP", field: "address", align: "left" },
  {
    name: "is_gateway",
    label: "Gateway",
    field: "is_gateway",
    align: "center"
  },
  { name: "status", label: "狀態／用途", field: "status", align: "left" },
  { name: "assignment", label: "指派對象", field: "address", align: "left" },
  { name: "actions", label: "操作", field: "address", align: "right" }
];

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

async function fetchIps() {
  loading.value = true;
  try {
    const page = await listSubnetIps(subnetId, {
      q: filters.value.q.trim() || undefined,
      status: filters.value.status ?? undefined,
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
  pagination: { page: number; rowsPerPage: number };
}

function onRequest(request: TableRequest) {
  pagination.value.page = request.pagination.page;
  pagination.value.rowsPerPage = request.pagination.rowsPerPage;
  void fetchIps();
}

function openAssignment(entry: IpEntry) {
  assignmentEntry.value = entry;
  assignmentOpen.value = true;
}

function onAssignmentSaved() {
  void fetchIps();
}

function confirmCancel(entry: IpEntry) {
  $q.dialog({
    title: "取消指派",
    message: `確定要取消 ${entry.address} 的指派？取消後該位址回到「可用」。`,
    cancel: true,
    persistent: true
  }).onOk(() => {
    void cancel(entry);
  });
}

async function cancel(entry: IpEntry) {
  try {
    await cancelAssignment(subnetId, entry.address);
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

  if (!isV6.value) {
    void fetchIps();
  }
});
</script>

<style scoped>
.ip-address {
  font-family: monospace;
}
</style>
