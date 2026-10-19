<template>
  <q-page padding>
    <div class="row items-center q-mb-md">
      <div class="text-h6">報廢資產</div>
    </div>

    <div class="row q-col-gutter-sm q-mb-md">
      <div class="col-12 col-md-6">
        <q-input
          ref="searchInputRef"
          v-model="filters.q"
          outlined
          dense
          clearable
          placeholder="搜尋財產編號／描述／位置／設備序號／廠牌／型號／備註"
          @update:model-value="onSearchInput"
        >
          <template #prepend>
            <q-icon name="search" />
          </template>
        </q-input>
      </div>
    </div>

    <q-table
      :rows="disposedAssets"
      :columns="columns"
      row-key="id"
      :loading="loading"
      v-model:pagination="pagination"
      :rows-per-page-options="[10, 25, 50]"
      @request="onRequest"
    >
      <template #body-cell-property_no="props">
        <q-td :props="props">{{ props.value || "—" }}</q-td>
      </template>
      <template #body-cell-device_serial="props">
        <q-td :props="props">{{ props.value || "—" }}</q-td>
      </template>
      <template #body-cell-brand="props">
        <q-td :props="props">{{ props.value || "—" }}</q-td>
      </template>
      <template #body-cell-model="props">
        <q-td :props="props">{{ props.value || "—" }}</q-td>
      </template>
      <template #body-cell-purchase_date="props">
        <q-td :props="props">{{ props.value || "—" }}</q-td>
      </template>
      <template #body-cell-actions="props">
        <q-td :props="props" class="text-right">
          <q-btn
            flat
            dense
            no-caps
            color="primary"
            label="復原"
            :loading="restoringId === props.row.id"
            :disable="restoringId !== null && restoringId !== props.row.id"
            @click="confirmRestore(props.row)"
          />
        </q-td>
      </template>
    </q-table>
  </q-page>
</template>

<script setup lang="ts">
import type { QInput, QTableProps } from "quasar";
import { useQuasar } from "quasar";
import { onBeforeUnmount, onMounted, ref } from "vue";

import {
  listDisposedAssets,
  restoreDisposedAsset,
  type DisposedAsset
} from "@/api/disposedAssets";
import { useCompositionGuard } from "@/composables/useCompositionGuard";
import { debounce } from "@/utils/debounce";
import { messageOf } from "@/utils/errors";

const $q = useQuasar();

const disposedAssets = ref<DisposedAsset[]>([]);
const loading = ref(false);

/** 搜尋輸入框的組字守衛（見 .scratch/ime-composition/spec.md）。 */
const searchInputRef = ref<QInput | null>(null);
useCompositionGuard(searchInputRef);

const filters = ref<{ q: string | null }>({ q: "" });

/** 伺服器端分頁；預設每頁 10，`rowsNumber` 取後端 `total`（見 spec §6）。 */
const pagination = ref({
  page: 1,
  rowsPerPage: 10,
  rowsNumber: 0
});

/** 進行中的復原 id（同一時間至多一筆；loading 防連點，比照借還頁「快速歸還」）。 */
const restoringId = ref<number | null>(null);

const columns: QTableProps["columns"] = [
  {
    name: "property_no",
    label: "財產編號",
    field: "property_no",
    align: "left"
  },
  { name: "description", label: "描述", field: "description", align: "left" },
  { name: "location", label: "位置", field: "location", align: "left" },
  {
    name: "device_serial",
    label: "設備序號",
    field: "device_serial",
    align: "left"
  },
  { name: "brand", label: "廠牌", field: "brand", align: "left" },
  { name: "model", label: "型號", field: "model", align: "left" },
  {
    name: "purchase_date",
    label: "購置日期",
    field: "purchase_date",
    align: "left"
  },
  {
    name: "disposal_date",
    label: "報廢日期",
    field: "disposal_date",
    align: "left"
  },
  { name: "actions", label: "操作", field: "id", align: "right" }
];

interface TableRequest {
  pagination: {
    page: number;
    rowsPerPage: number;
    sortBy: string | null;
    descending: boolean;
  };
}

async function fetchDisposedAssets() {
  loading.value = true;
  try {
    const page = await listDisposedAssets({
      q: filters.value.q?.trim() || undefined,
      page: pagination.value.page,
      per_page: pagination.value.rowsPerPage
    });

    disposedAssets.value = page.items;
    pagination.value.rowsNumber = page.total;
  } catch (cause) {
    $q.notify({ type: "negative", message: messageOf(cause) });
  } finally {
    loading.value = false;
  }
}

function reload() {
  pagination.value.page = 1;
  void fetchDisposedAssets();
}

/** 搜尋輸入去抖：q-input 即時 emit（見 .scratch/ime-composition/spec.md），查詢仍 300ms 去抖。 */
const searchDebounced = debounce(reload, 300);

function onSearchInput() {
  searchDebounced.call();
}

onBeforeUnmount(() => searchDebounced.cancel());

function onRequest(request: TableRequest) {
  const { page, rowsPerPage } = request.pagination;
  pagination.value.page = page;
  pagination.value.rowsPerPage = rowsPerPage;
  void fetchDisposedAssets();
}

/** 對話框以 HTML 呈現提示資訊；資產描述為使用者輸入，需轉義。 */
function escapeHtml(value: string): string {
  return value
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#39;");
}

/** 復原二次確認：介面與 IP 指派不會回復、需重新建立（見 spec §6）。 */
function confirmRestore(row: DisposedAsset) {
  $q.dialog({
    title: "復原資產",
    message:
      "復原後資產將回到資產清單；介面與 IP 指派不會回復，需重新建立。<br>" +
      `確定要復原「${escapeHtml(row.description)}」？`,
    html: true,
    cancel: true,
    persistent: true
  }).onOk(() => {
    void restore(row);
  });
}

/** 復原：成功通知「已復原資產」後重載（該列自清單消失；見 spec §2 不變量）。 */
async function restore(row: DisposedAsset) {
  if (restoringId.value !== null) {
    return;
  }
  restoringId.value = row.id;
  try {
    await restoreDisposedAsset(row.id);
    $q.notify({ type: "positive", message: "已復原資產" });
    await fetchDisposedAssets();
  } catch (cause) {
    $q.notify({ type: "negative", message: messageOf(cause) });
  } finally {
    restoringId.value = null;
  }
}

onMounted(() => {
  void fetchDisposedAssets();
});
</script>
