<template>
  <q-page padding>
    <div class="row items-center q-mb-md">
      <div class="text-h6">資產管理</div>
      <q-space />
      <q-btn color="primary" icon="add" label="新增資產" @click="openCreate" />
    </div>

    <div class="row q-col-gutter-sm q-mb-md">
      <div class="col-12 col-md-6">
        <q-input
          v-model="filters.q"
          outlined
          dense
          clearable
          debounce="300"
          placeholder="搜尋財產編號／描述／設備序號／廠牌／型號／備註"
          @update:model-value="reload"
        >
          <template #prepend>
            <q-icon name="search" />
          </template>
        </q-input>
      </div>
      <div class="col-12 col-sm-4 col-md-2">
        <q-select
          v-model="filters.location"
          :options="locationOptions"
          outlined
          dense
          clearable
          label="位置"
          @update:model-value="reload"
        />
      </div>
      <div class="col-12 col-sm-4 col-md-2">
        <q-select
          v-model="filters.brand"
          :options="brandOptions"
          outlined
          dense
          clearable
          label="廠牌"
          @update:model-value="reload"
        />
      </div>
      <div class="col-12 col-sm-4 col-md-2">
        <q-select
          v-model="filters.tag"
          :options="tagOptions"
          outlined
          dense
          clearable
          label="標籤"
          @update:model-value="reload"
        />
      </div>
    </div>

    <q-table
      :rows="assets"
      :columns="columns"
      row-key="id"
      :loading="loading"
      :pagination="pagination"
      :rows-per-page-options="[10, 25, 50]"
      @request="onRequest"
    >
      <template #body-cell-property_no="props">
        <q-td :props="props">{{ props.value || "—" }}</q-td>
      </template>
      <template #body-cell-brand="props">
        <q-td :props="props">{{ props.value || "—" }}</q-td>
      </template>
      <template #body-cell-model="props">
        <q-td :props="props">{{ props.value || "—" }}</q-td>
      </template>
      <template #body-cell-note="props">
        <q-td :props="props">{{ props.value || "—" }}</q-td>
      </template>
      <template #body-cell-tags="props">
        <q-td :props="props">
          <q-chip
            v-for="tag in props.row.tags"
            :key="tag"
            dense
            size="sm"
            color="primary"
            text-color="white"
          >
            {{ tag }}
          </q-chip>
          <span v-if="props.row.tags.length === 0">—</span>
        </q-td>
      </template>
      <template #body-cell-expired="props">
        <q-td :props="props" class="text-center">
          <q-badge v-if="props.row.expired" color="warning" text-color="black">
            屆齡
          </q-badge>
        </q-td>
      </template>
      <template #body-cell-actions="props">
        <q-td :props="props" class="text-right">
          <q-btn
            flat
            dense
            round
            icon="add_link"
            aria-label="指派 IP"
            @click="openAssign(props.row)"
          />
          <q-btn
            flat
            dense
            round
            icon="edit"
            aria-label="編輯"
            @click="openEdit(props.row)"
          />
          <q-btn
            flat
            dense
            round
            icon="delete"
            color="negative"
            aria-label="刪除"
            @click="confirmDelete(props.row)"
          />
        </q-td>
      </template>
    </q-table>

    <asset-form-dialog v-model="dialogOpen" :asset="editing" @saved="onSaved" />

    <assign-ip-dialog
      v-if="assignAsset !== null"
      v-model="assignOpen"
      :asset="assignAsset"
      @saved="onAssignSaved"
    />
  </q-page>
</template>

<script setup lang="ts">
import type { QTableProps } from "quasar";
import { useQuasar } from "quasar";
import { onMounted, ref } from "vue";

import {
  deleteAsset,
  fetchAsset,
  fetchBrands,
  fetchLocations,
  fetchTags,
  listAssets,
  type Asset,
  type AssetDetail
} from "@/api/assets";
import AssignIpDialog from "@/components/AssignIpDialog.vue";
import AssetFormDialog from "@/components/AssetFormDialog.vue";

const $q = useQuasar();

const assets = ref<Asset[]>([]);
const loading = ref(false);
const locationOptions = ref<string[]>([]);
const brandOptions = ref<string[]>([]);
const tagOptions = ref<string[]>([]);

const filters = ref<{
  q: string | null;
  location: string | null;
  brand: string | null;
  tag: string | null;
}>({
  q: "",
  location: null,
  brand: null,
  tag: null
});

/** 伺服器端分頁與排序；預設描述升冪（見 spec §6）。 */
const pagination = ref<{
  page: number;
  rowsPerPage: number;
  rowsNumber: number;
  sortBy: string | null;
  descending: boolean;
}>({
  page: 1,
  rowsPerPage: 50,
  rowsNumber: 0,
  sortBy: "description",
  descending: false
});

const columns: QTableProps["columns"] = [
  {
    name: "property_no",
    label: "財產編號",
    field: "property_no",
    align: "left",
    sortable: true
  },
  {
    name: "description",
    label: "描述",
    field: "description",
    align: "left",
    sortable: true
  },
  {
    name: "location",
    label: "位置",
    field: "location",
    align: "left",
    sortable: true
  },
  {
    name: "brand",
    label: "廠牌",
    field: "brand",
    align: "left",
    sortable: true
  },
  {
    name: "model",
    label: "型號",
    field: "model",
    align: "left",
    sortable: true
  },
  { name: "note", label: "備註", field: "note", align: "left", sortable: true },
  { name: "tags", label: "標籤", field: "tags", align: "left", sortable: true },
  {
    name: "expired",
    label: "屆齡",
    field: "expired",
    align: "center",
    sortable: true
  },
  { name: "actions", label: "操作", field: "id", align: "right" }
];

const dialogOpen = ref(false);
const editing = ref<Asset | null>(null);

const assignOpen = ref(false);
const assignAsset = ref<Asset | null>(null);

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

async function fetchAssets() {
  loading.value = true;
  try {
    const page = await listAssets({
      q: filters.value.q?.trim() || undefined,
      location: filters.value.location ?? undefined,
      brand: filters.value.brand ?? undefined,
      tag: filters.value.tag ?? undefined,
      sort: pagination.value.sortBy ?? undefined,
      dir: pagination.value.descending ? "desc" : "asc",
      page: pagination.value.page,
      per_page: pagination.value.rowsPerPage
    });

    assets.value = page.items;
    pagination.value.rowsNumber = page.total;
  } catch (cause) {
    $q.notify({ type: "negative", message: messageOf(cause) });
  } finally {
    loading.value = false;
  }
}

async function loadFilterOptions() {
  try {
    const [locations, brands, tags] = await Promise.all([
      fetchLocations(),
      fetchBrands(),
      fetchTags()
    ]);
    locationOptions.value = locations;
    brandOptions.value = brands;
    tagOptions.value = tags;
  } catch (cause) {
    $q.notify({ type: "negative", message: messageOf(cause) });
  }
}

function reload() {
  pagination.value.page = 1;
  void fetchAssets();
}

function onRequest(request: TableRequest) {
  const { page, rowsPerPage, sortBy, descending } = request.pagination;
  const sortChanged =
    sortBy !== pagination.value.sortBy ||
    descending !== pagination.value.descending;

  pagination.value.sortBy = sortBy;
  pagination.value.descending = descending;
  // 切換排序時回到第 1 頁（見票 11）
  pagination.value.page = sortChanged ? 1 : page;
  pagination.value.rowsPerPage = rowsPerPage;
  void fetchAssets();
}

function openCreate() {
  editing.value = null;
  dialogOpen.value = true;
}

function openEdit(asset: Asset) {
  editing.value = asset;
  dialogOpen.value = true;
}

function onSaved() {
  void fetchAssets();
  void loadFilterOptions();
}

/** 由資產列直接指派 IP（見票 10）；位址可能反推自任一網段。 */
function openAssign(asset: Asset) {
  assignAsset.value = asset;
  assignOpen.value = true;
}

function onAssignSaved() {
  void fetchAssets();
}

function confirmDelete(asset: Asset) {
  void confirmDeleteWithImpact(asset);
}

/** 先讀取連動影響數量（介面、指派、其中保留）再顯示確認；讀取失敗不進入刪除流程。 */
async function confirmDeleteWithImpact(asset: Asset) {
  let detail: AssetDetail;
  try {
    detail = await fetchAsset(asset.id);
  } catch (cause) {
    $q.notify({ type: "negative", message: messageOf(cause) });
    return;
  }

  const interfaceCount = detail.interfaces.length;
  const assignmentCount = detail.assignments.length;
  const reservationCount = detail.assignments.filter(
    item => item.purpose === "reservation"
  ).length;

  $q.dialog({
    title: "刪除資產",
    message:
      `將刪除 ${interfaceCount} 個介面、${assignmentCount} 筆指派` +
      `（含 ${reservationCount} 筆保留）。確定要刪除「${asset.description}」？`,
    cancel: true,
    persistent: true
  }).onOk(() => {
    void remove(asset);
  });
}

async function remove(asset: Asset) {
  try {
    await deleteAsset(asset.id);
    $q.notify({ type: "positive", message: "已刪除資產" });
    await fetchAssets();
    await loadFilterOptions();
  } catch (cause) {
    $q.notify({ type: "negative", message: messageOf(cause) });
  }
}

onMounted(() => {
  void fetchAssets();
  void loadFilterOptions();
});
</script>
