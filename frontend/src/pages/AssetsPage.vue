<template>
  <q-page class="assets-page">
    <div class="assets-shell">
      <page-header eyebrow="Asset registry" title="資產管理">
        <template #meta>
          <span class="an-pill">
            <q-icon name="inventory_2" size="15px" />
            {{ totalLabel }}
          </span>
          <span v-if="activeFilterCount > 0" class="an-pill an-pill--forest">
            <q-icon name="filter_alt" size="15px" />
            已套用 {{ activeFilterCount }} 個篩選
          </span>
        </template>

        <template #actions>
          <q-btn
            flat
            no-caps
            icon="download"
            label="匯出"
            :loading="exporting"
            @click="exportCsv"
          />
          <q-btn
            flat
            no-caps
            icon="upload_file"
            label="匯入"
            @click="importOpen = true"
          />
          <q-btn
            unelevated
            no-caps
            color="primary"
            icon="add"
            label="新增資產"
            @click="openCreate"
          />
        </template>
      </page-header>

      <section class="filter-panel" aria-label="搜尋與篩選">
        <div class="filter-search">
          <q-input
            ref="searchInputRef"
            v-model="filters.q"
            outlined
            dense
            clearable
            placeholder="搜尋財產編號、描述、序號、廠牌、型號、備註、MAC、IP"
            @update:model-value="onSearchInput"
          >
            <template #prepend>
              <q-icon name="search" />
            </template>
          </q-input>
        </div>

        <div class="filter-selects">
          <q-select
            v-model="filters.location"
            :options="locationOptions"
            outlined
            dense
            clearable
            label="位置"
            class="filter-select"
            @filter="filterLocations"
            @update:model-value="reload"
          />
          <q-select
            v-model="filters.brand"
            :options="brandOptions"
            outlined
            dense
            clearable
            label="廠牌"
            class="filter-select"
            @update:model-value="reload"
          />
          <q-select
            v-model="filters.tag"
            :options="tagOptions"
            outlined
            dense
            clearable
            label="標籤"
            class="filter-select"
            @update:model-value="reload"
          />
        </div>

        <div v-if="activeFilterCount > 0" class="filter-reset">
          <q-btn
            flat
            dense
            no-caps
            class="filter-reset__btn"
            icon="filter_alt_off"
            label="清除篩選"
            @click="clearFilters"
          />
        </div>
      </section>

      <section class="table-panel">
        <div class="table-head">
          <div>
            <h2 class="table-title">資產清冊</h2>
            <p class="table-subtitle">{{ rangeLabel }}</p>
          </div>
        </div>

        <div v-if="!loading && assets.length === 0" class="empty-state">
          <span class="empty-state__icon">
            <q-icon name="spa" size="30px" />
          </span>
          <p class="empty-state__title">
            {{
              activeFilterCount > 0
                ? "找不到符合條件的資產"
                : "資產清冊還是空的"
            }}
          </p>
          <p class="empty-state__text">
            {{
              activeFilterCount > 0
                ? "換個關鍵字，或清除篩選條件再試一次。"
                : "建立第一筆資產，開始累積設備清冊。"
            }}
          </p>
          <q-btn
            v-if="activeFilterCount > 0"
            unelevated
            no-caps
            color="primary"
            class="empty-state__btn"
            label="清除篩選"
            @click="clearFilters"
          />
          <q-btn
            v-else
            unelevated
            no-caps
            color="primary"
            class="empty-state__btn"
            icon="add"
            label="新增資產"
            @click="openCreate"
          />
        </div>

        <q-table
          v-else
          class="assets-table"
          flat
          :rows="assets"
          :columns="columns"
          row-key="id"
          :loading="loading"
          v-model:pagination="pagination"
          :rows-per-page-options="[10, 25, 50]"
          binary-state-sort
          :grid="$q.screen.lt.md"
          hide-bottom
          @request="onRequest"
        >
          <template #body-cell-property_no="props">
            <q-td :props="props">
              <span v-if="props.value" class="cell-property">
                {{ props.value }}
              </span>
              <span v-else class="cell-empty">—</span>
            </q-td>
          </template>

          <template #body-cell-description="props">
            <q-td :props="props">
              <div class="asset-desc">
                <span class="asset-desc__text">{{
                  props.row.description
                }}</span>
                <span v-if="isLent(props.row)" class="asset-flag">
                  <q-icon name="swap_horiz" size="13px" />
                  出借中 · {{ props.row.lending.borrower }}
                </span>
              </div>
            </q-td>
          </template>

          <template #body-cell-location="props">
            <q-td :props="props">
              <span class="cell-location">{{ props.value || "—" }}</span>
            </q-td>
          </template>

          <template #body-cell-brand="props">
            <q-td :props="props">
              <span class="cell-muted">{{ props.value || "—" }}</span>
            </q-td>
          </template>

          <template #body-cell-model="props">
            <q-td :props="props">
              <span class="cell-muted">{{ props.value || "—" }}</span>
            </q-td>
          </template>

          <template #body-cell-note="props">
            <q-td :props="props">
              <span v-if="props.value" class="cell-note" :title="props.value">
                {{ props.value }}
              </span>
              <span v-else class="cell-empty">—</span>
            </q-td>
          </template>

          <template #body-cell-assigned_ips="props">
            <q-td :props="props">
              <div v-if="props.row.assigned_ips.length > 0" class="ip-list">
                <span
                  v-for="address in visibleIps(props.row)"
                  :key="address"
                  class="ip-chip"
                >
                  {{ address }}
                </span>
                <span
                  v-if="extraIps(props.row).length > 0"
                  class="ip-more"
                  tabindex="0"
                >
                  +{{ extraIps(props.row).length }}
                  <q-tooltip>{{ extraIps(props.row).join("、") }}</q-tooltip>
                </span>
              </div>
              <span v-else class="cell-empty">—</span>
            </q-td>
          </template>

          <!-- 最後可見（見票 08）：無命中現況顯示「—」，其餘為相對時間。 -->
          <template #body-cell-last_seen="props">
            <q-td :props="props">
              <span
                v-if="props.row.last_seen_at"
                class="seen"
                :class="`seen--${seenTone(props.row.last_seen_at)}`"
              >
                <span class="seen-dot" aria-hidden="true"></span>
                {{ relativeTime(props.row.last_seen_at) }}
                <q-tooltip>
                  最後觀測：{{ absoluteTime(props.row.last_seen_at) }}
                </q-tooltip>
              </span>
              <span v-else class="cell-empty">—</span>
            </q-td>
          </template>

          <template #body-cell-tags="props">
            <q-td :props="props">
              <div v-if="props.row.tags.length > 0" class="tag-list">
                <span
                  v-for="tag in visibleTags(props.row)"
                  :key="tag"
                  class="tag-chip"
                >
                  {{ tag }}
                </span>
                <span
                  v-if="extraTags(props.row).length > 0"
                  class="tag-more"
                  tabindex="0"
                >
                  +{{ extraTags(props.row).length }}
                  <q-tooltip>{{ extraTags(props.row).join("、") }}</q-tooltip>
                </span>
              </div>
              <span v-else class="cell-empty">—</span>
            </q-td>
          </template>

          <template #body-cell-expired="props">
            <q-td :props="props" class="text-center">
              <span
                v-if="props.row.expired"
                class="status-badge status-badge--due"
              >
                屆齡
              </span>
              <span v-else class="cell-empty">—</span>
            </q-td>
          </template>

          <template #body-cell-actions="props">
            <q-td :props="props" class="text-right">
              <div class="row-actions">
                <q-btn
                  v-if="!isLent(props.row)"
                  flat
                  dense
                  round
                  icon="person_add"
                  aria-label="借出"
                  @click="openLend(props.row)"
                >
                  <q-tooltip>借出</q-tooltip>
                </q-btn>
                <q-btn
                  flat
                  dense
                  round
                  icon="add_link"
                  aria-label="指派 IP"
                  @click="openAssign(props.row)"
                >
                  <q-tooltip>指派 IP</q-tooltip>
                </q-btn>
                <q-btn
                  flat
                  dense
                  round
                  icon="edit"
                  aria-label="編輯"
                  @click="openEdit(props.row)"
                >
                  <q-tooltip>編輯</q-tooltip>
                </q-btn>
                <span>
                  <q-btn
                    flat
                    dense
                    round
                    icon="delete"
                    class="row-actions__danger"
                    aria-label="刪除"
                    :disable="isLent(props.row)"
                    @click="confirmDelete(props.row)"
                  >
                    <q-tooltip v-if="isLent(props.row)">
                      出借中，請先歸還
                    </q-tooltip>
                    <q-tooltip v-else>刪除</q-tooltip>
                  </q-btn>
                </span>
              </div>
            </q-td>
          </template>

          <!-- 行動版卡片（`grid` 模式；見 spec §6 響應式設計） -->
          <template #item="props">
            <article class="asset-card">
              <header class="asset-card__head">
                <span class="asset-card__no">
                  {{ props.row.property_no || "未編號" }}
                </span>
                <span class="asset-card__flags">
                  <span
                    v-if="props.row.expired"
                    class="status-badge status-badge--due"
                  >
                    屆齡
                  </span>
                  <span
                    v-if="isLent(props.row)"
                    class="status-badge status-badge--lent"
                  >
                    出借中
                  </span>
                </span>
              </header>
              <h3 class="asset-card__title">{{ props.row.description }}</h3>
              <p v-if="isLent(props.row)" class="asset-card__lent">
                借用人：{{ props.row.lending.borrower }}
              </p>
              <dl class="asset-card__facts">
                <div>
                  <dt>位置</dt>
                  <dd>{{ props.row.location || "—" }}</dd>
                </div>
                <div>
                  <dt>廠牌／型號</dt>
                  <dd>{{ brandModel(props.row) }}</dd>
                </div>
                <div>
                  <dt>已指派 IP</dt>
                  <dd>
                    <div
                      v-if="props.row.assigned_ips.length > 0"
                      class="ip-list"
                    >
                      <span
                        v-for="address in props.row.assigned_ips"
                        :key="address"
                        class="ip-chip"
                      >
                        {{ address }}
                      </span>
                    </div>
                    <span v-else class="cell-empty">—</span>
                  </dd>
                </div>
                <div>
                  <dt>最後可見</dt>
                  <dd>
                    <span
                      v-if="props.row.last_seen_at"
                      class="seen"
                      :class="`seen--${seenTone(props.row.last_seen_at)}`"
                    >
                      <span class="seen-dot" aria-hidden="true"></span>
                      {{ relativeTime(props.row.last_seen_at) }}
                    </span>
                    <span v-else class="cell-empty">—</span>
                  </dd>
                </div>
              </dl>
              <div v-if="props.row.tags.length > 0" class="tag-list">
                <span v-for="tag in props.row.tags" :key="tag" class="tag-chip">
                  {{ tag }}
                </span>
              </div>
              <footer class="asset-card__actions">
                <q-btn
                  v-if="!isLent(props.row)"
                  flat
                  dense
                  no-caps
                  icon="person_add"
                  label="借出"
                  @click="openLend(props.row)"
                />
                <q-btn
                  flat
                  dense
                  no-caps
                  icon="add_link"
                  label="指派 IP"
                  @click="openAssign(props.row)"
                />
                <q-btn
                  flat
                  dense
                  no-caps
                  icon="edit"
                  label="編輯"
                  @click="openEdit(props.row)"
                />
                <span>
                  <q-btn
                    flat
                    dense
                    no-caps
                    icon="delete"
                    label="刪除"
                    class="row-actions__danger"
                    :disable="isLent(props.row)"
                    @click="confirmDelete(props.row)"
                  />
                  <q-tooltip v-if="isLent(props.row)">
                    出借中，請先歸還
                  </q-tooltip>
                </span>
              </footer>
            </article>
          </template>
        </q-table>

        <footer v-if="pagination.rowsNumber > 0" class="table-footer">
          <label class="table-footer__size">
            每頁
            <q-select
              v-model="pagination.rowsPerPage"
              :options="[10, 25, 50]"
              dense
              borderless
              options-dense
              class="page-size"
              aria-label="每頁筆數"
              @update:model-value="onPageSizeChange"
            />
            筆
          </label>

          <div class="table-footer__pager">
            <q-btn
              flat
              dense
              round
              icon="first_page"
              aria-label="第一頁"
              :disable="pagination.page <= 1"
              @click="goToPage(1)"
            />
            <q-btn
              flat
              dense
              round
              icon="chevron_left"
              aria-label="上一頁"
              :disable="pagination.page <= 1"
              @click="goToPage(pagination.page - 1)"
            />
            <span class="page-indicator">
              {{ pagination.page }} / {{ pageCount }}
            </span>
            <q-btn
              flat
              dense
              round
              icon="chevron_right"
              aria-label="下一頁"
              :disable="pagination.page >= pageCount"
              @click="goToPage(pagination.page + 1)"
            />
            <q-btn
              flat
              dense
              round
              icon="last_page"
              aria-label="最後一頁"
              :disable="pagination.page >= pageCount"
              @click="goToPage(pageCount)"
            />
          </div>
        </footer>
      </section>
    </div>

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
import type { QInput, QTableProps } from "quasar";
import { useQuasar } from "quasar";
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { useRoute } from "vue-router";

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
import type { LendingBrief } from "@/api/lendings";
import AssignIpDialog from "@/components/AssignIpDialog.vue";
import AssetFormDialog from "@/components/AssetFormDialog.vue";
import AssetImportDialog from "@/components/AssetImportDialog.vue";
import LendingDialog from "@/components/LendingDialog.vue";
import PageHeader from "@/components/PageHeader.vue";
import { useCompositionGuard } from "@/composables/useCompositionGuard";
import { summarizeAssetImpact } from "@/utils/assetImpact";
import { debounce } from "@/utils/debounce";
import { messageOf } from "@/utils/errors";
import { notifyKeaSync } from "@/utils/keaSync";
import { relativeTime } from "@/utils/relativeTime";

const $q = useQuasar();
const route = useRoute();

const assets = ref<AssetListRow[]>([]);
const loading = ref(false);

/** 搜尋輸入框的組字守衛（見 .scratch/ime-composition/spec.md）。 */
const searchInputRef = ref<QInput | null>(null);
useCompositionGuard(searchInputRef);
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

/** 目前套用的篩選數量（搜尋字串、位置、廠牌、標籤；空白搜尋不計）。 */
const activeFilterCount = computed(() => {
  let count = 0;
  if ((filters.value.q ?? "").trim() !== "") {
    count += 1;
  }
  if (filters.value.location !== null) {
    count += 1;
  }
  if (filters.value.brand !== null) {
    count += 1;
  }
  if (filters.value.tag !== null) {
    count += 1;
  }
  return count;
});

/** 目前清單總筆數（有篩選時為符合條件的筆數）。 */
const totalLabel = computed(() => {
  const total = pagination.value.rowsNumber.toLocaleString("zh-TW");
  return activeFilterCount.value > 0
    ? `符合條件 ${total} 筆`
    : `共 ${total} 筆`;
});

/** 目前頁數（伺服器端分頁總頁數；至少 1）。 */
const pageCount = computed(() =>
  Math.max(
    1,
    Math.ceil(pagination.value.rowsNumber / pagination.value.rowsPerPage)
  )
);

/** 切換頁碼：沿用表格的伺服器端請求流程（`@request` 由 q-table 發出）。 */
function goToPage(page: number) {
  const target = Math.min(Math.max(1, page), pageCount.value);
  if (target === pagination.value.page) {
    return;
  }
  pagination.value.page = target;
  void fetchAssets();
}

/** 切換每頁筆數：回到第一頁後重載。 */
function onPageSizeChange() {
  pagination.value.page = 1;
  void fetchAssets();
}

/** 表格上方目前顯示範圍（例：顯示第 1–50 筆 · 共 1,743 筆）。 */
const rangeLabel = computed(() => {
  const total = pagination.value.rowsNumber;
  if (total === 0) {
    return activeFilterCount.value > 0
      ? "沒有符合篩選條件的資產"
      : "尚未建立任何資產";
  }
  const start = (pagination.value.page - 1) * pagination.value.rowsPerPage + 1;
  const end = Math.min(total, start + assets.value.length - 1);
  return `顯示第 ${start.toLocaleString("zh-TW")}–${end.toLocaleString(
    "zh-TW"
  )} 筆 · 共 ${total.toLocaleString("zh-TW")} 筆`;
});

const columns: QTableProps["columns"] = [
  {
    name: "property_no",
    label: "財產編號",
    field: "property_no",
    align: "left",
    sortable: true,
    classes: "col-property",
    headerClasses: "col-property",
    style: "width: 170px"
  },
  {
    name: "description",
    label: "描述",
    field: "description",
    align: "left",
    sortable: true,
    classes: "col-description",
    headerClasses: "col-description",
    style: "width: 200px"
  },
  {
    name: "location",
    label: "位置",
    field: "location",
    align: "left",
    sortable: true,
    classes: "col-location",
    headerClasses: "col-location",
    style: "width: 120px"
  },
  {
    // 依第一筆已指派位址排序（v4 先、v6 後、同族數值；未指派固定最後；見票 19）
    name: "assigned_ips",
    label: "已指派 IP",
    field: "assigned_ips",
    align: "left",
    sortable: true,
    classes: "col-ips",
    headerClasses: "col-ips",
    style: "width: 170px"
  },
  {
    // 依命中的觀測現況最大值排序（NULL 固定最後；見票 08）
    name: "last_seen",
    label: "最後可見",
    field: (row: AssetListRow) => row.last_seen_at ?? "",
    align: "left",
    sortable: true,
    classes: "col-seen",
    headerClasses: "col-seen",
    style: "width: 110px"
  },
  {
    name: "brand",
    label: "廠牌",
    field: "brand",
    align: "left",
    sortable: true,
    classes: "col-brand",
    headerClasses: "col-brand",
    style: "width: 92px"
  },
  {
    name: "model",
    label: "型號",
    field: "model",
    align: "left",
    sortable: true,
    classes: "col-model",
    headerClasses: "col-model",
    style: "width: 120px"
  },
  {
    name: "note",
    label: "備註",
    field: "note",
    align: "left",
    sortable: true,
    classes: "col-note",
    headerClasses: "col-note",
    style: "width: 160px"
  },
  {
    name: "tags",
    label: "標籤",
    field: "tags",
    align: "left",
    sortable: true,
    classes: "col-tags",
    headerClasses: "col-tags",
    style: "width: 130px"
  },
  {
    name: "expired",
    label: "屆齡",
    field: "expired",
    align: "center",
    sortable: true,
    classes: "col-expired",
    headerClasses: "col-expired",
    style: "width: 70px"
  },
  {
    name: "actions",
    label: "操作",
    field: "id",
    align: "right",
    classes: "col-actions",
    headerClasses: "col-actions",
    style: "width: 150px"
  }
];

const dialogOpen = ref(false);
/** 編輯中的資產：資產清單列帶出借摘要，供對話框停用報廢（見 asset-disposal spec §5）。 */
type EditingAsset = Asset & { lending?: LendingBrief | null };
const editing = ref<EditingAsset | null>(null);

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

/** 清除全部搜尋與篩選條件，回到第一頁。 */
function clearFilters() {
  filters.value = { q: "", location: null, brand: null, tag: null };
  reload();
}

/** 表格內最多顯示 2 個 IP，其餘收進「+N」提示（避免列高被撐開）。 */
function visibleIps(row: AssetListRow): string[] {
  return row.assigned_ips.slice(0, 2);
}

function extraIps(row: AssetListRow): string[] {
  return row.assigned_ips.slice(2);
}

/** 表格內最多顯示 2 個標籤，其餘收進「+N」提示。 */
function visibleTags(row: AssetListRow): string[] {
  return row.tags.slice(0, 2);
}

function extraTags(row: AssetListRow): string[] {
  return row.tags.slice(2);
}

/** 廠牌與型號的合併顯示（行動版卡片用）。 */
function brandModel(row: AssetListRow): string {
  return [row.brand, row.model].filter(Boolean).join(" ") || "—";
}

/** 最後可見的視覺分級：1 小時內、24 小時內、更久。 */
function seenTone(value: string): "fresh" | "stale" | "old" {
  const time = Date.parse(value);
  if (Number.isNaN(time)) {
    return "old";
  }
  const age = Date.now() - time;
  if (age < 60 * 60 * 1000) {
    return "fresh";
  }
  if (age < 24 * 60 * 60 * 1000) {
    return "stale";
  }
  return "old";
}

/** 最後可見的絕對時間（tooltip 用；本地時區）。 */
function absoluteTime(value: string): string {
  const date = new Date(value);
  return Number.isNaN(date.getTime())
    ? value
    : date.toLocaleString("zh-TW", { hour12: false });
}

/** 搜尋輸入去抖：q-input 即時 emit（見 .scratch/ime-composition/spec.md），查詢仍 300ms 去抖。 */
const searchDebounced = debounce(reload, 300);

function onSearchInput() {
  searchDebounced.call();
}

onBeforeUnmount(() => searchDebounced.cancel());

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

function openEdit(asset: EditingAsset) {
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

  const impact = summarizeAssetImpact({
    interfaces: detail.interfaces,
    assignments: detail.assignments,
    lastSeenAt: detail.last_seen_at
  });

  $q.dialog({
    title: "刪除資產",
    message:
      `將刪除 ${impact.interfaceCount} 個介面、${impact.assignmentCount} 筆指派` +
      `（含 ${impact.reservationCount} 筆保留）。<br>` +
      `最後可見：${impact.lastSeen}；介面 MAC：${impact.macLabel}。<br>` +
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
    const result = await deleteAsset(asset.id);
    notifyKeaSync(
      $q,
      result.kea_sync.find(sync => sync.status === "failed")
    );
    $q.notify({ type: "positive", message: "已刪除資產" });
    await fetchAssets();
    await loadFilterOptions();
  } catch (cause) {
    $q.notify({ type: "negative", message: messageOf(cause) });
  }
}

onMounted(() => {
  // 「清冊稽核」的「在資產清單檢視」以 `?q=` 帶入初始搜尋字串（見清冊稽核 spec §6）
  const initialQuery = route.query.q;
  if (typeof initialQuery === "string" && initialQuery !== "") {
    filters.value.q = initialQuery;
  }
  void fetchAssets();
  void loadFilterOptions();
});
</script>

<style scoped>
/* 資產管理頁樣式：Organic Biophilic + Minimalism
   （頁面範圍設計權杖；不影響其他頁面。見 .scratch/ui-refresh/spec.md） */

.assets-shell {
  display: flex;
  flex-direction: column;
  gap: clamp(14px, 1.8vw, 22px);
  width: 100%;
  max-width: 1560px;
  margin: 0 auto;
  padding: clamp(16px, 2.4vw, 34px);
}

/* ── 篩選列 ─────────────────────────────── */

.filter-panel {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 12px;
  padding: 14px 16px;
  background: var(--an-surface);
  border: 1px solid var(--an-line-soft);
  border-radius: var(--an-radius-lg);
  box-shadow: var(--an-shadow);
}

.filter-search {
  flex: 1 1 320px;
  min-width: 240px;
}

.filter-selects {
  display: flex;
  flex-wrap: wrap;
  gap: 10px;
}

.filter-select {
  min-width: 148px;
}

.filter-reset__btn {
  color: var(--an-clay);
  text-transform: none;
}

.filter-panel :deep(.q-field--outlined .q-field__control) {
  border-radius: var(--an-radius-sm);
  background: var(--an-surface-soft);
  transition: background-color 0.2s ease;
}

.filter-panel :deep(.q-field--outlined.q-field--focused .q-field__control) {
  background: #ffffff;
}

.filter-panel :deep(.q-field__label),
.filter-panel :deep(.q-field__marginal) {
  color: var(--an-muted);
}

/* ── 表格容器 ───────────────────────────── */

.table-panel {
  overflow: hidden;
  background: var(--an-surface);
  border: 1px solid var(--an-line-soft);
  border-radius: var(--an-radius-lg);
  box-shadow: var(--an-shadow);
}

.table-head {
  display: flex;
  align-items: flex-end;
  justify-content: space-between;
  gap: 16px;
  padding: 18px 24px 10px;
}

.table-title {
  margin: 0;
  font-size: 16px;
  font-weight: 600;
  line-height: 1.4;
  letter-spacing: 0.06em;
  color: var(--an-ink);
}

.table-subtitle {
  margin: 5px 0 0;
  font-size: 12.5px;
  color: var(--an-muted);
}

/* ── 表格（桌機）────────────────────────── */

.assets-table {
  background: transparent;
}

.assets-table :deep(.q-table__middle) {
  background: transparent;
}

.assets-table :deep(thead tr th) {
  height: 44px;
  font-size: 12px;
  font-weight: 600;
  letter-spacing: 0.09em;
  color: var(--an-muted);
  background: transparent;
  border-bottom: 1px solid var(--an-line);
}

.assets-table :deep(tbody tr) {
  transition: background-color 0.2s ease;
}

.assets-table :deep(tbody tr:hover) {
  background: rgba(126, 155, 130, 0.09);
}

.assets-table :deep(tbody td) {
  height: 58px;
  font-size: 13.5px;
  color: var(--an-ink-soft);
  white-space: nowrap;
  border-bottom: 1px solid var(--an-line-soft);
}

.assets-table :deep(tbody tr:last-child td) {
  border-bottom: none;
}

.assets-table :deep(.q-table__bottom) {
  padding: 8px 18px;
  color: var(--an-muted);
  border-top: 1px solid var(--an-line-soft);
}

.assets-table :deep(.q-table__sort-icon) {
  color: var(--an-sage);
}

/* 欄位內容樣式 */

.cell-property {
  font-family: "Roboto Mono", ui-monospace, SFMono-Regular, Menlo, monospace;
  font-size: 12.5px;
  letter-spacing: 0.02em;
  color: var(--an-ink);
}

.cell-location {
  color: var(--an-ink-soft);
}

.cell-muted {
  color: var(--an-muted);
}

.cell-note {
  display: inline-block;
  max-width: 170px;
  overflow: hidden;
  color: var(--an-muted);
  text-overflow: ellipsis;
  vertical-align: bottom;
}

.cell-empty {
  color: #b7bdb4;
}

.asset-desc {
  display: flex;
  flex-direction: column;
  gap: 3px;
}

.asset-desc__text {
  font-weight: 500;
  color: var(--an-ink);
}

.asset-flag {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  font-size: 11.5px;
  color: var(--an-clay);
}

.ip-list {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
  align-items: center;
}

.ip-chip {
  padding: 2px 8px;
  font-family: "Roboto Mono", ui-monospace, SFMono-Regular, Menlo, monospace;
  font-size: 12px;
  color: var(--an-forest);
  background: var(--an-sage-soft);
  border: 1px solid rgba(47, 93, 70, 0.14);
  border-radius: 8px;
}

.ip-more,
.tag-more {
  padding: 2px 7px;
  font-size: 11.5px;
  color: var(--an-muted);
  cursor: help;
  background: var(--an-surface-soft);
  border: 1px solid var(--an-line);
  border-radius: 999px;
}

.ip-more:focus-visible,
.tag-more:focus-visible {
  outline: 2px solid var(--an-forest);
  outline-offset: 2px;
}

.tag-list {
  display: flex;
  flex-wrap: wrap;
  gap: 5px;
}

.tag-chip {
  padding: 2px 10px;
  font-size: 12px;
  color: var(--an-forest);
  background: rgba(126, 155, 130, 0.16);
  border-radius: 999px;
}

.seen {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  color: var(--an-ink-soft);
}

.seen-dot {
  width: 7px;
  height: 7px;
  border-radius: 50%;
  background: #b7bdb4;
}

.seen--fresh .seen-dot {
  background: #4c8c5a;
  box-shadow: 0 0 0 3px rgba(76, 140, 90, 0.16);
}

.seen--stale .seen-dot {
  background: #cfa05a;
  box-shadow: 0 0 0 3px rgba(207, 160, 90, 0.16);
}

.seen--old .seen-dot {
  background: #b7bdb4;
}

.status-badge {
  display: inline-flex;
  align-items: center;
  padding: 2px 10px;
  font-size: 11.5px;
  font-weight: 500;
  border-radius: 999px;
}

.status-badge--due {
  color: #8c4a2f;
  background: var(--an-clay-soft);
}

.status-badge--lent {
  color: #8a6a2e;
  background: #f7efd9;
}

/* 列內操作 */

.row-actions {
  display: inline-flex;
  gap: 2px;
  justify-content: flex-end;
  opacity: 0.72;
  transition: opacity 0.2s ease;
}

.assets-table :deep(tbody tr:hover) .row-actions,
.assets-table :deep(tbody tr:focus-within) .row-actions {
  opacity: 1;
}

.row-actions :deep(.q-btn) {
  color: var(--an-muted);
}

.row-actions :deep(.q-btn:hover) {
  color: var(--an-forest);
  background: var(--an-sage-soft);
}

.row-actions :deep(.row-actions__danger) {
  color: var(--an-brick);
}

/* ── 行動版卡片（grid 模式）──────────────── */

.assets-table :deep(.q-table__grid-content) {
  padding: 6px 12px 16px;
}

.assets-table :deep(.q-table__grid-item) {
  padding: 6px;
}

.assets-table :deep(.q-table__grid-item-card) {
  height: 100%;
  padding: 18px;
  background: var(--an-surface);
  border: 1px solid var(--an-line-soft);
  border-radius: var(--an-radius-md);
  box-shadow: 0 16px 34px -30px rgba(31, 42, 36, 0.55);
  transition:
    border-color 0.2s ease,
    box-shadow 0.2s ease;
}

.assets-table :deep(.q-table__grid-item-card:hover) {
  border-color: rgba(47, 93, 70, 0.22);
  box-shadow: 0 18px 38px -28px rgba(31, 42, 36, 0.5);
}

.asset-card {
  display: flex;
  flex-direction: column;
  gap: 12px;
  height: 100%;
}

.asset-card__head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
}

.asset-card__no {
  font-family: "Roboto Mono", ui-monospace, SFMono-Regular, Menlo, monospace;
  font-size: 12px;
  color: var(--an-muted);
}

.asset-card__flags {
  display: inline-flex;
  gap: 6px;
}

.asset-card__title {
  margin: 0;
  font-size: 16px;
  font-weight: 600;
  line-height: 1.45;
  color: var(--an-ink);
}

.asset-card__lent {
  margin: -4px 0 0;
  font-size: 12.5px;
  color: var(--an-clay);
}

.asset-card__facts {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 10px 14px;
  margin: 0;
}

.asset-card__facts dt {
  margin-bottom: 3px;
  font-size: 11.5px;
  letter-spacing: 0.08em;
  color: var(--an-muted);
}

.asset-card__facts dd {
  margin: 0;
  font-size: 13px;
  line-height: 1.5;
  color: var(--an-ink-soft);
  overflow-wrap: anywhere;
}

.asset-card__actions {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
  padding-top: 10px;
  margin-top: auto;
  border-top: 1px solid var(--an-line-soft);
}

.asset-card__actions :deep(.q-btn) {
  color: var(--an-muted);
  text-transform: none;
}

.asset-card__actions :deep(.q-btn:hover) {
  color: var(--an-forest);
  background: var(--an-sage-soft);
}

.asset-card__actions :deep(.row-actions__danger) {
  color: var(--an-brick);
}

/* ── 空狀態 ─────────────────────────────── */

.empty-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 8px;
  padding: clamp(32px, 5vw, 56px) 24px;
  text-align: center;
}

.empty-state__icon {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 64px;
  height: 64px;
  margin-bottom: 6px;
  color: var(--an-forest);
  background: var(--an-sage-soft);
  border-radius: 46% 54% 58% 42% / 52% 46% 54% 48%;
}

.empty-state__title {
  margin: 0;
  font-size: 16px;
  font-weight: 600;
  color: var(--an-ink);
}

.empty-state__text {
  max-width: 320px;
  margin: 0;
  font-size: 13px;
  line-height: 1.7;
  color: var(--an-muted);
}

.empty-state__btn {
  margin-top: 10px;
  text-transform: none;
  border-radius: 999px;
}

/* ── 表格頁尾（自訂分頁）──────────────── */

.table-footer {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 10px 20px 14px;
  border-top: 1px solid var(--an-line-soft);
}

.table-footer__size {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  font-size: 12.5px;
  color: var(--an-muted);
}

.page-size {
  width: 74px;
}

.page-size :deep(.q-field__native) {
  justify-content: flex-end;
}

.table-footer__pager {
  display: inline-flex;
  align-items: center;
  gap: 2px;
}

.page-indicator {
  min-width: 64px;
  font-size: 12.5px;
  color: var(--an-muted);
  text-align: center;
}

.table-footer :deep(.q-btn) {
  color: var(--an-muted);
}

.table-footer :deep(.q-btn:hover) {
  color: var(--an-forest);
  background: var(--an-sage-soft);
}

/* ── 響應式 ─────────────────────────────── */

@media (max-width: 1700px) {
  .assets-table :deep(.col-note) {
    display: none;
  }
}

@media (max-width: 1550px) {
  .assets-table :deep(.col-model) {
    display: none;
  }
}

@media (max-width: 1440px) {
  .assets-table :deep(.col-brand) {
    display: none;
  }
}

@media (max-width: 1330px) {
  .assets-table :deep(.col-tags) {
    display: none;
  }
}

@media (max-width: 1023px) {
  .filter-selects {
    flex: 1 1 100%;
  }

  .filter-select {
    flex: 1 1 140px;
  }
}

@media (max-width: 599px) {
  .assets-shell {
    padding: 12px;
  }

  .filter-panel,
  .table-panel {
    border-radius: 20px;
  }

  .table-head {
    padding: 16px 18px 8px;
  }

  .asset-card__facts {
    grid-template-columns: minmax(0, 1fr);
  }
}

@media (prefers-reduced-motion: reduce) {
  .assets-page *,
  .assets-page *::before,
  .assets-page *::after {
    transition-duration: 0.01ms !important;
    animation-duration: 0.01ms !important;
    animation-iteration-count: 1 !important;
  }
}
</style>
