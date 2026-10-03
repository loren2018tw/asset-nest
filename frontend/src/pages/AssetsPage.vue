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
      <div class="col-12 col-sm-6 col-md-3">
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
      <div class="col-12 col-sm-6 col-md-3">
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
  </q-page>
</template>

<script setup lang="ts">
import type { QTableProps } from "quasar";
import { useQuasar } from "quasar";
import { onMounted, ref } from "vue";

import {
  deleteAsset,
  fetchBrands,
  fetchLocations,
  listAssets,
  type Asset
} from "@/api/assets";
import AssetFormDialog from "@/components/AssetFormDialog.vue";

const $q = useQuasar();

const assets = ref<Asset[]>([]);
const loading = ref(false);
const locationOptions = ref<string[]>([]);
const brandOptions = ref<string[]>([]);

const filters = ref<{
  q: string;
  location: string | null;
  brand: string | null;
}>({
  q: "",
  location: null,
  brand: null
});

const pagination = ref({ page: 1, rowsPerPage: 50, rowsNumber: 0 });

const columns: QTableProps["columns"] = [
  {
    name: "property_no",
    label: "財產編號",
    field: "property_no",
    align: "left"
  },
  { name: "description", label: "描述", field: "description", align: "left" },
  { name: "location", label: "位置", field: "location", align: "left" },
  { name: "brand", label: "廠牌", field: "brand", align: "left" },
  { name: "model", label: "型號", field: "model", align: "left" },
  { name: "note", label: "備註", field: "note", align: "left" },
  { name: "expired", label: "屆齡", field: "expired", align: "center" },
  { name: "actions", label: "操作", field: "id", align: "right" }
];

const dialogOpen = ref(false);
const editing = ref<Asset | null>(null);

interface TableRequest {
  pagination: { page: number; rowsPerPage: number };
}

function messageOf(cause: unknown): string {
  return cause instanceof Error ? cause.message : String(cause);
}

async function fetchAssets() {
  loading.value = true;
  try {
    const page = await listAssets({
      q: filters.value.q.trim() || undefined,
      location: filters.value.location ?? undefined,
      brand: filters.value.brand ?? undefined,
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
    const [locations, brands] = await Promise.all([
      fetchLocations(),
      fetchBrands()
    ]);
    locationOptions.value = locations;
    brandOptions.value = brands;
  } catch (cause) {
    $q.notify({ type: "negative", message: messageOf(cause) });
  }
}

function reload() {
  pagination.value.page = 1;
  void fetchAssets();
}

function onRequest(request: TableRequest) {
  pagination.value.page = request.pagination.page;
  pagination.value.rowsPerPage = request.pagination.rowsPerPage;
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

function confirmDelete(asset: Asset) {
  $q.dialog({
    title: "刪除資產",
    message: `確定要刪除「${asset.description}」？`,
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
