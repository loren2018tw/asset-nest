<template>
  <q-page padding>
    <div class="row items-center q-mb-md">
      <div class="text-h6">資產借還</div>
    </div>

    <div class="text-subtitle1 q-mb-sm">出借中</div>
    <q-table
      :rows="openLendings"
      :columns="openColumns"
      row-key="id"
      :loading="openLoading"
      :pagination="openPagination"
      hide-bottom
      no-data-label="目前沒有出借中的資產"
    >
      <template #body-cell-lent_at="props">
        <q-td :props="props">{{ formatTime(props.row.lent_at) }}</q-td>
      </template>
      <template #body-cell-due_at="props">
        <q-td :props="props">
          {{ props.row.due_at ?? "—" }}
          <q-chip
            v-if="props.row.overdue"
            dense
            size="sm"
            color="negative"
            text-color="white"
            class="q-ml-sm"
          >
            逾期
          </q-chip>
        </q-td>
      </template>
      <template #body-cell-actions="props">
        <q-td :props="props" class="text-right">
          <q-btn
            flat
            dense
            no-caps
            color="primary"
            label="快速歸還"
            :loading="returningId === props.row.id"
            :disable="returningId !== null && returningId !== props.row.id"
            @click="quickReturn(props.row)"
          />
        </q-td>
      </template>
    </q-table>

    <div class="text-subtitle1 q-mb-sm q-mt-lg">已歸還紀錄</div>
    <q-table
      :rows="returnedRows"
      :columns="returnedColumns"
      row-key="id"
      :loading="returnedLoading"
      v-model:pagination="returnedPagination"
      :rows-per-page-options="[10, 25, 50]"
      @request="onReturnedRequest"
    >
      <template #body-cell-lent_at="props">
        <q-td :props="props">{{ formatTime(props.row.lent_at) }}</q-td>
      </template>
      <template #body-cell-returned_at="props">
        <q-td :props="props">{{ formatTime(props.row.returned_at) }}</q-td>
      </template>
      <template #body-cell-due_at="props">
        <q-td :props="props">{{ props.row.due_at ?? "—" }}</q-td>
      </template>
    </q-table>
  </q-page>
</template>

<script setup lang="ts">
import type { QTableProps } from "quasar";
import { useQuasar } from "quasar";
import { onMounted, ref } from "vue";

import {
  listOpenLendings,
  listReturnedLendings,
  returnLending,
  type LendingWithAsset
} from "@/api/lendings";
import { assetLabel } from "@/utils/assetLabel";

const $q = useQuasar();

const openLendings = ref<LendingWithAsset[]>([]);
const openLoading = ref(false);
/** 出借中不分頁全列：`rowsPerPage: 0` 即全部顯示（Quasar q-table 慣例）。 */
const openPagination = { page: 1, rowsPerPage: 0 };

const returnedRows = ref<LendingWithAsset[]>([]);
const returnedLoading = ref(false);
/** 伺服器端分頁；預設每頁 10，`rowsNumber` 取後端 `total`（見 spec §6.2）。 */
const returnedPagination = ref({
  page: 1,
  rowsPerPage: 10,
  rowsNumber: 0
});

/** 進行中的快速歸還 id（同一時間至多一筆；loading 防連點）。 */
const returningId = ref<number | null>(null);

const openColumns: QTableProps["columns"] = [
  {
    name: "asset",
    label: "資產",
    field: (row: LendingWithAsset) =>
      assetLabel(row.property_no, row.description),
    align: "left"
  },
  { name: "borrower", label: "借用人", field: "borrower", align: "left" },
  { name: "lent_at", label: "借出時間", field: "lent_at", align: "left" },
  { name: "due_at", label: "預計歸還日", field: "due_at", align: "left" },
  { name: "actions", label: "操作", field: "id", align: "right" }
];

const returnedColumns: QTableProps["columns"] = [
  {
    name: "asset",
    label: "資產",
    field: (row: LendingWithAsset) =>
      assetLabel(row.property_no, row.description),
    align: "left"
  },
  { name: "borrower", label: "借用人", field: "borrower", align: "left" },
  { name: "lent_at", label: "借出時間", field: "lent_at", align: "left" },
  {
    name: "returned_at",
    label: "歸還時間",
    field: "returned_at",
    align: "left"
  },
  { name: "due_at", label: "預計歸還日", field: "due_at", align: "left" }
];

interface TableRequest {
  pagination: {
    page: number;
    rowsPerPage: number;
    sortBy: string | null;
    descending: boolean;
  };
}

function messageOf(cause: unknown): string {
  return cause instanceof Error ? cause.message : String(cause);
}

/** 借出／歸還時間（UTC ISO8601）轉本地時間顯示；null 顯示「—」、無法解析原樣。 */
function formatTime(value: string | null): string {
  if (value === null) {
    return "—";
  }
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) {
    return value;
  }
  return date.toLocaleString("zh-TW", { hour12: false });
}

async function fetchOpen() {
  openLoading.value = true;
  try {
    openLendings.value = await listOpenLendings();
  } catch (cause) {
    $q.notify({ type: "negative", message: messageOf(cause) });
  } finally {
    openLoading.value = false;
  }
}

async function fetchReturned() {
  returnedLoading.value = true;
  try {
    const page = await listReturnedLendings({
      page: returnedPagination.value.page,
      per_page: returnedPagination.value.rowsPerPage
    });
    returnedRows.value = page.items;
    returnedPagination.value.rowsNumber = page.total;
  } catch (cause) {
    $q.notify({ type: "negative", message: messageOf(cause) });
  } finally {
    returnedLoading.value = false;
  }
}

function onReturnedRequest(request: TableRequest) {
  const { page, rowsPerPage } = request.pagination;
  returnedPagination.value.page = page;
  returnedPagination.value.rowsPerPage = rowsPerPage;
  void fetchReturned();
}

/** 快速歸還：一鍵呼叫、不彈確認；成功 notify「已歸還」後重取上下兩區塊。 */
async function quickReturn(lending: LendingWithAsset) {
  if (returningId.value !== null) {
    return;
  }
  returningId.value = lending.id;
  try {
    await returnLending(lending.id);
    $q.notify({ type: "positive", message: "已歸還" });
    await Promise.all([fetchOpen(), fetchReturned()]);
  } catch (cause) {
    $q.notify({ type: "negative", message: messageOf(cause) });
  } finally {
    returningId.value = null;
  }
}

onMounted(() => {
  void fetchOpen();
  void fetchReturned();
});
</script>
