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
            placeholder="搜尋 IP（完整位址精確比對；否則子字串）"
            @update:model-value="reload"
          >
            <template #prepend>
              <q-icon name="search" />
            </template>
          </q-input>
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
            <q-badge :color="props.row.in_pool ? 'blue-grey' : 'positive'">
              {{ props.row.in_pool ? "池內" : "可用" }}
            </q-badge>
            <span v-if="props.row.purpose" class="q-ml-sm">
              {{ purposeLabel(props.row.purpose) }}
            </span>
          </q-td>
        </template>
        <template #body-cell-assignment="props">
          <!-- 指派對象（資產＋介面）為票 05；本票唯讀顯示預留欄位 -->
          <q-td :props="props">—</q-td>
        </template>
        <template #body-cell-actions="props">
          <q-td :props="props" class="text-right">
            <!-- 票 05 於此提供編輯／指派；池內列依 spec §7 無編輯入口 -->
            <q-btn
              v-if="!props.row.in_pool"
              flat
              dense
              round
              icon="edit"
              disable
              aria-label="指派（票 05）"
            >
              <q-tooltip>指派功能將於票 05 提供</q-tooltip>
            </q-btn>
          </q-td>
        </template>
      </q-table>
    </template>
  </q-page>
</template>

<script setup lang="ts">
import type { QTableProps } from "quasar";
import { useQuasar } from "quasar";
import { computed, onMounted, ref } from "vue";
import { useRoute } from "vue-router";

import { listSubnetIps, type IpEntry } from "@/api/ips";
import { fetchSubnet, type Subnet } from "@/api/subnets";

const $q = useQuasar();
const route = useRoute();

const subnetId = Number(route.params.id);
const subnet = ref<Subnet | null>(null);
const ips = ref<IpEntry[]>([]);
const loading = ref(false);
const filters = ref<{ q: string }>({ q: "" });
const pagination = ref({ page: 1, rowsPerPage: 50, rowsNumber: 0 });

/** v6 為登錄制、尚未支援瀏覽（見票 06），不呼叫 IP API。 */
const isV6 = computed(() => subnet.value?.cidr.includes(":") ?? false);

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

/** 指派用途標籤；本票尚無指派資料，僅為票 05 預留。 */
function purposeLabel(purpose: string | null): string {
  if (purpose === "static") {
    return "手動設定";
  }
  if (purpose === "reservation") {
    return "保留";
  }
  return purpose ?? "—";
}

async function fetchIps() {
  loading.value = true;
  try {
    const page = await listSubnetIps(subnetId, {
      q: filters.value.q.trim() || undefined,
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
