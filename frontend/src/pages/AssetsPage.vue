<template>
  <q-page padding>
    <div class="row items-center q-mb-md">
      <div class="text-h6">資產管理</div>
      <q-space />
      <q-btn
        color="primary"
        outline
        icon="download"
        label="匯出"
        :loading="exporting"
        @click="exportCsv"
      />
      <q-btn
        color="primary"
        outline
        icon="upload_file"
        label="匯入"
        @click="importOpen = true"
      />
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
          placeholder="搜尋財產編號／描述／設備序號／廠牌／型號／備註／MAC／已指派 IP"
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
          use-input
          label="位置"
          @filter="filterLocations"
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
      v-model:pagination="pagination"
      :rows-per-page-options="[10, 25, 50]"
      binary-state-sort
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
      <template #body-cell-assigned_ips="props">
        <q-td :props="props">
          <q-chip
            v-for="address in props.row.assigned_ips"
            :key="address"
            dense
            size="sm"
          >
            {{ address }}
          </q-chip>
          <span v-if="props.row.assigned_ips.length === 0">—</span>
        </q-td>
      </template>
      <!-- 最後可見（見票 08）：無命中現況顯示「—」，其餘為相對時間。 -->
      <template #body-cell-last_seen="props">
        <q-td :props="props">
          <span v-if="props.row.last_seen_at">
            {{ relativeTime(props.row.last_seen_at) }}
          </span>
          <span v-else class="text-grey-6">—</span>
        </q-td>
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
          <!-- 出借中：橘色停用按鈕＋借用人 tooltip；tooltip 掛在不被停用的外層 span
               （停用按鈕不觸發滑鼠事件，tooltip 須以外層元素為錨點） -->
          <span v-if="isLent(props.row)">
            <q-btn flat dense no-caps color="orange" label="出借中" disable />
            <q-tooltip>出借中：{{ props.row.lending.borrower }}</q-tooltip>
          </span>
          <q-btn
            v-else
            flat
            dense
            round
            icon="person_add"
            aria-label="借出"
            @click="openLend(props.row)"
          />
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
          <span>
            <q-btn
              flat
              dense
              round
              icon="delete"
              color="negative"
              aria-label="刪除"
              :disable="isLent(props.row)"
              @click="confirmDelete(props.row)"
            />
            <q-tooltip v-if="isLent(props.row)"> 出借中，請先歸還 </q-tooltip>
          </span>
        </q-td>
      </template>
    </q-table>

    <asset-form-dialog v-model="dialogOpen" :asset="editing" @saved="onSaved" />

    <asset-import-dialog v-model="importOpen" @saved="onSaved" />

    <assign-ip-dialog
      v-if="assignAsset !== null"
      v-model="assignOpen"
      :asset="assignAsset"
      @saved="onAssignSaved"
    />

    <lending-dialog
      v-if="lendAsset !== null"
      v-model="lendOpen"
      :asset="lendAsset"
      @saved="onLendSaved"
    />
  </q-page>
</template>

<script setup lang="ts">
import type { QTableProps } from "quasar";
import { useQuasar } from "quasar";
import { onMounted, ref } from "vue";

import {
  deleteAsset,
  downloadAssetsCsv,
  fetchAsset,
  fetchBrands,
  fetchLocations,
  fetchTags,
  listAssets,
  type Asset,
  type AssetDetail,
  type AssetListRow,
  type AssetSortField
} from "@/api/assets";
import { saveBlob } from "@/api/client";
import AssignIpDialog from "@/components/AssignIpDialog.vue";
import AssetFormDialog from "@/components/AssetFormDialog.vue";
import AssetImportDialog from "@/components/AssetImportDialog.vue";
import LendingDialog from "@/components/LendingDialog.vue";
import { relativeTime } from "@/utils/relativeTime";

const $q = useQuasar();

const assets = ref<AssetListRow[]>([]);
const loading = ref(false);
/** 全部位置選項；位置篩選的本地過濾來源。 */
const allLocationOptions = ref<string[]>([]);
/** 依輸入過濾後顯示的位置選項。 */
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

/**
 * 伺服器端分頁與排序；預設描述升冪（見 spec §6）。
 * 須以 `v-model:pagination` 綁定（而非只傳 `:pagination`），Quasar 才會把
 * 父層更新併回表格內部狀態，箭頭顯示與點擊反向切換才正常（見票 19）。
 * `binary-state-sort`：同欄點擊只在 asc／desc 間切換，不停在「取消排序」。
 */
const pagination = ref<{
  page: number;
  rowsPerPage: number;
  rowsNumber: number;
  sortBy: AssetSortField;
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
    // 依第一筆已指派位址排序（v4 先、v6 後、同族數值；未指派固定最後；見票 19）
    name: "assigned_ips",
    label: "已指派 IP",
    field: "assigned_ips",
    align: "left",
    sortable: true
  },
  {
    // 依命中的觀測現況最大值排序（NULL 固定最後；見票 08）
    name: "last_seen",
    label: "最後可見",
    field: (row: AssetListRow) => row.last_seen_at ?? "",
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

const importOpen = ref(false);
const exporting = ref(false);

const assignOpen = ref(false);
const assignAsset = ref<Asset | null>(null);

const lendOpen = ref(false);
const lendAsset = ref<Asset | null>(null);

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

/** 後端未帶檔名時的預設匯出檔名（本機日期，格式 `資產匯出_YYYYMMDD.csv`）。 */
function exportFilename(date = new Date()): string {
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  return `資產匯出_${year}${month}${day}.csv`;
}

/** 匯出目前搜尋／篩選／排序的全部資產（忽略分頁；見 spec §4）。 */
async function exportCsv() {
  exporting.value = true;
  try {
    const { blob, filename } = await downloadAssetsCsv({
      q: filters.value.q?.trim() || undefined,
      location: filters.value.location ?? undefined,
      brand: filters.value.brand ?? undefined,
      tag: filters.value.tag ?? undefined,
      sort: pagination.value.sortBy ?? undefined,
      dir: pagination.value.descending ? "desc" : "asc"
    });
    saveBlob(blob, filename ?? exportFilename());
    $q.notify({ type: "positive", message: "已下載資產匯出檔" });
  } catch (cause) {
    $q.notify({ type: "negative", message: messageOf(cause) });
  } finally {
    exporting.value = false;
  }
}

async function loadFilterOptions() {
  try {
    const [locations, brands, tags] = await Promise.all([
      fetchLocations(),
      fetchBrands(),
      fetchTags()
    ]);
    allLocationOptions.value = locations;
    locationOptions.value = locations;
    brandOptions.value = brands;
    tagOptions.value = tags;
  } catch (cause) {
    $q.notify({ type: "negative", message: messageOf(cause) });
  }
}

/** 依輸入本地即時過濾既有位置（大小寫無關、子字串）；輸入不觸發伺服器查詢。 */
function filterLocations(
  input: string,
  update: (callback: () => void) => void
) {
  const needle = input.toLowerCase();
  update(() => {
    locationOptions.value = allLocationOptions.value.filter(location =>
      location.toLowerCase().includes(needle)
    );
  });
}

function reload() {
  pagination.value.page = 1;
  void fetchAssets();
}

/** q-table 的排序欄位名即欄位 `name`；僅白名單欄位可送後端（見票 08）。 */
function toSortField(value: string | null): AssetSortField {
  switch (value) {
    case "property_no":
    case "description":
    case "location":
    case "assigned_ips":
    case "brand":
    case "model":
    case "note":
    case "tags":
    case "expired":
    case "last_seen":
      return value;
    default:
      return "description";
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

/** 該列是否出借中：`lending` 摘要存在即出借中（見 asset-lending spec §4）。 */
function isLent(row: AssetListRow): boolean {
  return row.lending !== null && row.lending !== undefined;
}

/** 由資產列直接借出（見 asset-lending spec §5）；未出借才顯示按鈕。 */
function openLend(asset: Asset) {
  lendAsset.value = asset;
  lendOpen.value = true;
}

function onLendSaved() {
  void fetchAssets();
}

function confirmDelete(asset: Asset) {
  void confirmDeleteWithImpact(asset);
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

/**
 * 先讀取連動影響數量（介面、指派、其中保留）再顯示確認；讀取失敗不進入刪除流程。
 * 另顯示該資產的最後可見與介面 MAC 作為回收防呆提示（僅提示、不阻擋；
 * 見票 08、ADR-0014）。
 */
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

  const lastSeen =
    detail.last_seen_at === null ? "—" : relativeTime(detail.last_seen_at);
  const macs = detail.interfaces
    .map(item => item.mac)
    .filter((mac): mac is string => mac !== null && mac !== "");
  const macLabel = macs.length > 0 ? macs.join("、") : "—";

  $q.dialog({
    title: "刪除資產",
    message:
      `將刪除 ${interfaceCount} 個介面、${assignmentCount} 筆指派` +
      `（含 ${reservationCount} 筆保留）。<br>` +
      `最後可見：${lastSeen}；介面 MAC：${macLabel}。<br>` +
      `僅提示，不阻擋刪除。確定要刪除「${escapeHtml(asset.description)}」？`,
    html: true,
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
