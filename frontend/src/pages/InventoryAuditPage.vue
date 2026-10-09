<template>
  <q-page padding>
    <div class="row items-center q-mb-md">
      <div class="text-h6">清冊稽核</div>
      <q-space />
      <q-btn
        v-if="phase === 'upload'"
        color="primary"
        icon="fact_check"
        label="開始稽核"
        :loading="auditing"
        :disable="!canAudit"
        @click="startAudit"
      />
      <q-btn
        v-else
        color="primary"
        outline
        icon="restart_alt"
        label="重新上傳"
        @click="resetAudit"
      />
    </div>

    <!-- ① 上傳（檔級錯誤時於上方顯示錯誤區塊；見 spec §6） -->
    <template v-if="phase === 'upload'">
      <q-banner
        v-if="errorMessage !== ''"
        dense
        rounded
        class="bg-negative text-white q-mb-md"
      >
        <template #avatar>
          <q-icon name="error" />
        </template>
        <div class="text-subtitle2">無法稽核：整批未執行</div>
        <div class="q-mt-xs">{{ errorMessage }}</div>
        <ul v-if="fileErrors.length > 0" class="q-my-none q-pl-lg">
          <li
            v-for="(file, index) in fileErrors"
            :key="`${file.filename}-${index}`"
          >
            {{ displayFilename(file.filename) }}：{{ file.message }}
          </li>
        </ul>
      </q-banner>

      <div class="row q-col-gutter-md">
        <div class="col-12 col-md-7">
          <q-uploader
            class="full-width"
            label="選擇財管盤點清冊（.xls／.xlsx，可多選）"
            accept=".xls,.xlsx"
            multiple
            :auto-upload="false"
            flat
            bordered
            hide-upload-btn
            @added="onFilesAdded"
            @removed="onFilesRemoved"
            @rejected="onFilesRejected"
          />
          <div class="text-caption text-grey-7 q-mt-sm">
            {{ selectionLabel }}
          </div>
        </div>

        <div class="col-12 col-md-5">
          <q-list dense class="text-body2">
            <q-item>
              <q-item-section avatar>
                <q-icon name="upload_file" color="primary" />
              </q-item-section>
              <q-item-section>
                一次上傳 1～10 個清冊檔（.xls／.xlsx）
              </q-item-section>
            </q-item>
            <q-item>
              <q-item-section avatar>
                <q-icon name="straighten" color="primary" />
              </q-item-section>
              <q-item-section> 每檔上限 5 MB／5,000 資料列 </q-item-section>
            </q-item>
            <q-item>
              <q-item-section avatar>
                <q-icon name="rule" color="primary" />
              </q-item-section>
              <q-item-section>
                整批全有全無：任一檔有檔級錯誤，整次不稽核
              </q-item-section>
            </q-item>
            <q-item>
              <q-item-section avatar>
                <q-icon name="visibility_off" color="primary" />
              </q-item-section>
              <q-item-section>
                唯讀：檔案僅在後端記憶體解析，不寫入資料、不落地
              </q-item-section>
            </q-item>
            <q-item>
              <q-item-section avatar>
                <q-icon name="pin_drop" color="primary" />
              </q-item-section>
              <q-item-section>
                以「財產編號」比對系統資產；清冊原文不清洗、不折疊
              </q-item-section>
            </q-item>
          </q-list>
        </div>
      </div>
    </template>

    <!-- ② 結果（見 spec §6） -->
    <template v-else>
      <div class="row q-gutter-sm q-mb-md">
        <q-chip dense square color="grey-3" text-color="black">
          清冊合計 {{ summary.list_total }}（財產
          {{ summary.list_property }}＋物品 {{ summary.list_item }}）
        </q-chip>
        <q-chip dense square color="grey-3" text-color="black">
          系統資產 {{ summary.system_total }}
        </q-chip>
        <q-chip dense square color="positive" text-color="white">
          相符 {{ summary.matched }}
        </q-chip>
        <q-chip
          dense
          square
          :color="summary.missing_in_system > 0 ? 'warning' : 'grey-3'"
          text-color="black"
        >
          系統缺少 {{ summary.missing_in_system }}
        </q-chip>
        <q-chip
          dense
          square
          :color="summary.missing_in_list > 0 ? 'negative' : 'grey-3'"
          :text-color="summary.missing_in_list > 0 ? 'white' : 'black'"
        >
          清冊缺少 {{ summary.missing_in_list }}
        </q-chip>
      </div>

      <q-card flat bordered class="q-mb-md">
        <q-card-section class="q-py-sm">
          <div class="text-subtitle2 q-mb-sm">檔案摘要</div>
          <div
            v-for="(file, index) in fileSummaries"
            :key="`${file.filename}-${index}`"
            class="row items-center q-gutter-sm q-py-xs"
          >
            <q-icon name="description" color="grey-7" />
            <span class="text-body2">{{ file.filename }}</span>
            <q-badge :color="kindColor(file.kind)">
              {{ kindLabel(file.kind) }}
            </q-badge>
            <span class="text-caption text-grey-7">
              資料列 {{ file.total }}・略過 {{ file.skipped }}
            </span>
          </div>
        </q-card-section>
      </q-card>

      <q-card flat bordered class="q-mb-md">
        <q-card-section class="q-py-sm">
          <div class="row items-center q-gutter-sm q-mb-sm">
            <div class="text-subtitle2">清冊有、系統沒有</div>
            <div class="text-caption text-grey-7">
              {{
                countLabel(
                  filteredMissingInSystem.length,
                  missingInSystemRows.length
                )
              }}
            </div>
            <q-space />
            <q-input
              v-model="missingInSystemKeyword"
              dense
              outlined
              clearable
              placeholder="關鍵字篩選"
              style="width: 240px"
            >
              <template #prepend>
                <q-icon name="search" />
              </template>
            </q-input>
            <q-btn
              flat
              outline
              color="primary"
              icon="download"
              label="下載 CSV"
              :disable="filteredMissingInSystem.length === 0"
              @click="downloadMissingInSystem"
            />
          </div>

          <q-table
            :rows="filteredMissingInSystem"
            :columns="missingInSystemColumns"
            :row-key="missingInSystemRowKey"
            dense
            flat
            bordered
            no-wrap
            hide-bottom
            virtual-scroll
            :pagination="{ rowsPerPage: 0 }"
            style="height: 42vh; min-height: 280px"
          >
            <template #body-cell-kind="props">
              <q-td :props="props" class="text-center">
                <q-badge :color="kindColor(props.row.kind)">
                  {{ kindLabel(props.row.kind) }}
                </q-badge>
              </q-td>
            </template>
            <template #body-cell-property_no="props">
              <q-td :props="props" class="cell-pre-wrap">
                {{ props.value }}
              </q-td>
            </template>
            <template #body-cell-duplicate="props">
              <q-td :props="props" class="text-center">
                <q-badge
                  v-if="props.row.duplicate"
                  color="warning"
                  text-color="black"
                >
                  同鍵重複
                </q-badge>
              </q-td>
            </template>
            <template #no-data>
              <div class="full-width row flex-center text-grey-7 q-pa-md">
                {{
                  missingInSystemRows.length === 0
                    ? "沒有差異：清冊列皆存在於系統"
                    : "沒有符合關鍵字的列"
                }}
              </div>
            </template>
          </q-table>
        </q-card-section>
      </q-card>

      <q-card flat bordered class="q-mb-md">
        <q-card-section class="q-py-sm">
          <div class="row items-center q-gutter-sm q-mb-sm">
            <div class="text-subtitle2">系統有、清冊沒有</div>
            <div class="text-caption text-grey-7">
              {{
                countLabel(
                  filteredMissingInList.length,
                  missingInListRows.length
                )
              }}
            </div>
            <q-space />
            <q-input
              v-model="missingInListKeyword"
              dense
              outlined
              clearable
              placeholder="關鍵字篩選"
              style="width: 240px"
            >
              <template #prepend>
                <q-icon name="search" />
              </template>
            </q-input>
            <q-btn
              flat
              outline
              color="primary"
              icon="download"
              label="下載 CSV"
              :disable="filteredMissingInList.length === 0"
              @click="downloadMissingInList"
            />
          </div>

          <q-table
            :rows="filteredMissingInList"
            :columns="missingInListColumns"
            row-key="asset_id"
            dense
            flat
            bordered
            no-wrap
            hide-bottom
            virtual-scroll
            :pagination="{ rowsPerPage: 0 }"
            style="height: 42vh; min-height: 280px"
          >
            <template #body-cell-property_no="props">
              <q-td :props="props" class="cell-pre-wrap">
                {{ props.value }}
              </q-td>
            </template>
            <template #body-cell-duplicate="props">
              <q-td :props="props" class="text-center">
                <q-badge
                  v-if="props.row.duplicate"
                  color="warning"
                  text-color="black"
                >
                  同鍵重複
                </q-badge>
              </q-td>
            </template>
            <template #body-cell-actions="props">
              <q-td :props="props" class="text-right">
                <q-btn
                  flat
                  dense
                  no-caps
                  color="primary"
                  icon="visibility"
                  label="在資產清單檢視"
                  :to="{ path: '/assets', query: { q: props.row.property_no } }"
                />
              </q-td>
            </template>
            <template #no-data>
              <div class="full-width row flex-center text-grey-7 q-pa-md">
                {{
                  missingInListRows.length === 0
                    ? "沒有差異：系統資產皆有對應清冊列"
                    : "沒有符合關鍵字的列"
                }}
              </div>
            </template>
          </q-table>
        </q-card-section>
      </q-card>
    </template>
  </q-page>
</template>

<script setup lang="ts">
import type { QRejectedEntry, QTableProps } from "quasar";
import { useQuasar } from "quasar";
import { computed, ref } from "vue";

import { ApiError } from "@/api/client";
import {
  auditInventory,
  fileErrorsOf,
  type InventoryAuditFileError,
  type InventoryAuditKind,
  type InventoryAuditMissingInListRow,
  type InventoryAuditMissingInSystemRow,
  type InventoryAuditReport,
  type InventoryAuditSummary
} from "@/api/inventoryAudit";
import {
  downloadMissingInListCsv,
  downloadMissingInSystemCsv,
  kindLabel
} from "@/utils/inventoryAudit";

const $q = useQuasar();

/** 頁面階段：上傳（可含檔級錯誤區塊）／結果（見 spec §6）。 */
const phase = ref<"upload" | "result">("upload");
/** 稽核請求中（按鈕 loading；期間不可重送）。 */
const auditing = ref(false);
/** 檔級錯誤的頂層訊息（後端 `message`；網路錯誤為通用訊息）。 */
const errorMessage = ref("");
/** 檔級錯誤逐檔原因（後端 `details.files`；見 spec §4）。 */
const fileErrors = ref<InventoryAuditFileError[]>([]);
const report = ref<InventoryAuditReport | null>(null);

/** 已選清冊檔（由 QUploader 事件同步；即請求送出內容）。 */
const selectedFiles = ref<File[]>([]);

/** 兩表關鍵字篩選（前端子字串比對、不分大小寫）。 */
const missingInSystemKeyword = ref<string | null>(null);
const missingInListKeyword = ref<string | null>(null);

const EMPTY_SUMMARY: InventoryAuditSummary = {
  list_total: 0,
  list_property: 0,
  list_item: 0,
  system_total: 0,
  matched: 0,
  missing_in_system: 0,
  missing_in_list: 0
};

const summary = computed(() => report.value?.summary ?? EMPTY_SUMMARY);
const fileSummaries = computed(() => report.value?.files ?? []);
const missingInSystemRows = computed(
  () => report.value?.missing_in_system ?? []
);
const missingInListRows = computed(() => report.value?.missing_in_list ?? []);

const canAudit = computed(
  () => selectedFiles.value.length > 0 && !auditing.value
);

/** 已選檔案摘要（無檔案時提示；大小比照選擇檔案的顯示慣例）。 */
const selectionLabel = computed(() => {
  const files = selectedFiles.value;
  if (files.length === 0) {
    return "尚未選擇檔案";
  }
  const bytes = files.reduce((sum, file) => sum + file.size, 0);
  return `已選 ${files.length} 檔（共 ${fileSizeLabel(bytes)}）`;
});

const filteredMissingInSystem = computed(() =>
  filterRows(
    missingInSystemRows.value,
    missingInSystemKeyword.value,
    missingInSystemHaystack
  )
);

const filteredMissingInList = computed(() =>
  filterRows(
    missingInListRows.value,
    missingInListKeyword.value,
    missingInListHaystack
  )
);

const missingInSystemColumns: QTableProps["columns"] = [
  { name: "kind", label: "來源", field: "kind", align: "center" },
  { name: "file", label: "檔案", field: "file", align: "left" },
  {
    name: "row_number",
    label: "列號",
    field: "row_number",
    align: "right"
  },
  {
    name: "property_no",
    label: "財產編號",
    field: "property_no",
    align: "left"
  },
  {
    name: "name",
    label: "財產名稱",
    field: (row: InventoryAuditMissingInSystemRow) => cellText(row.name),
    align: "left"
  },
  {
    name: "form",
    label: "形式品名",
    field: (row: InventoryAuditMissingInSystemRow) => cellText(row.form),
    align: "left"
  },
  {
    name: "location",
    label: "存放地點",
    field: (row: InventoryAuditMissingInSystemRow) => cellText(row.location),
    align: "left"
  },
  {
    name: "custodian",
    label: "保管人員",
    field: (row: InventoryAuditMissingInSystemRow) => cellText(row.custodian),
    align: "left"
  },
  {
    name: "note",
    label: "備註",
    field: (row: InventoryAuditMissingInSystemRow) => cellText(row.note),
    align: "left"
  },
  {
    name: "duplicate",
    label: "同鍵重複",
    field: "duplicate",
    align: "center"
  }
];

const missingInListColumns: QTableProps["columns"] = [
  {
    name: "property_no",
    label: "財產編號",
    field: "property_no",
    align: "left"
  },
  {
    name: "description",
    label: "描述",
    field: "description",
    align: "left"
  },
  { name: "location", label: "位置", field: "location", align: "left" },
  {
    name: "note",
    label: "備註",
    field: (row: InventoryAuditMissingInListRow) => cellText(row.note),
    align: "left"
  },
  {
    name: "duplicate",
    label: "同鍵重複",
    field: "duplicate",
    align: "center"
  },
  { name: "actions", label: "操作", field: "asset_id", align: "right" }
];

/** QUploader 的檔案即原生 `File`；以事件同步已選清單（見 AssetImportDialog 慣例）。 */
function onFilesAdded(files: readonly File[]) {
  selectedFiles.value = [...selectedFiles.value, ...files];
  clearErrors();
}

function onFilesRemoved(files: readonly File[]) {
  selectedFiles.value = selectedFiles.value.filter(
    file => !files.includes(file)
  );
  clearErrors();
}

/** 瀏覽器端拒收（非 `.xls`／`.xlsx` 或超過 QUploader 內建限制）：警告提示。 */
function onFilesRejected(entries: QRejectedEntry[]) {
  if (entries.length === 0) {
    return;
  }
  const names = entries.map(entry => entry.file.name).join("、");
  const onlyAccept = entries.every(
    entry => entry.failedPropValidation === "accept"
  );
  const message = onlyAccept
    ? `僅接受 .xls／.xlsx 檔：${names}`
    : `檔案未通過檢查：${names}`;
  $q.notify({ type: "warning", message, timeout: 6000 });
}

/** 送出路徑：`POST /api/v1/inventory-audit`（multipart、`file` 多值）。 */
async function startAudit() {
  if (!canAudit.value) {
    return;
  }
  auditing.value = true;
  clearErrors();
  try {
    report.value = await auditInventory(selectedFiles.value);
    missingInSystemKeyword.value = null;
    missingInListKeyword.value = null;
    phase.value = "result";
  } catch (cause) {
    errorMessage.value = messageOf(cause);
    fileErrors.value =
      cause instanceof ApiError ? fileErrorsOf(cause.details) : [];
    $q.notify({ type: "negative", message: errorMessage.value, timeout: 8000 });
  } finally {
    auditing.value = false;
  }
}

/** 重新上傳：清除結果回到上傳步驟（uploader 因 v-if 重建、佇列清空）。 */
function resetAudit() {
  phase.value = "upload";
  report.value = null;
  selectedFiles.value = [];
  missingInSystemKeyword.value = null;
  missingInListKeyword.value = null;
  clearErrors();
}

function clearErrors() {
  errorMessage.value = "";
  fileErrors.value = [];
}

/** 前端子字串篩選：關鍵字不分大小寫；空關鍵字回全部列。 */
function filterRows<T>(
  rows: T[],
  keyword: string | null,
  haystack: (row: T) => string
): T[] {
  const needle = (keyword ?? "").trim().toLowerCase();
  if (needle === "") {
    return rows;
  }
  return rows.filter(row => haystack(row).toLowerCase().includes(needle));
}

/** 篩選欄位＝表格所有顯示文字（原文，含換行與空白）。 */
function missingInSystemHaystack(
  row: InventoryAuditMissingInSystemRow
): string {
  return [
    kindLabel(row.kind),
    row.file,
    String(row.row_number),
    row.property_no,
    row.name,
    row.form,
    row.location,
    row.custodian,
    row.note
  ]
    .filter((value): value is string => value !== null)
    .join("\n");
}

function missingInListHaystack(row: InventoryAuditMissingInListRow): string {
  return [row.property_no, row.description, row.location, row.note]
    .filter((value): value is string => value !== null)
    .join("\n");
}

/** 匯出目前顯示列（含篩選；「操作」欄不入 CSV；見 spec §6）。 */
function downloadMissingInSystem() {
  downloadMissingInSystemCsv(filteredMissingInSystem.value);
  $q.notify({ type: "positive", message: "已下載「系統缺少」CSV" });
}

function downloadMissingInList() {
  downloadMissingInListCsv(filteredMissingInList.value);
  $q.notify({ type: "positive", message: "已下載「清冊缺少」CSV" });
}

function missingInSystemRowKey(row: InventoryAuditMissingInSystemRow): string {
  return `${row.file}\u0000${row.row_number}`;
}

/** 筆數標示：未篩選＝共 N 筆；篩選中＝顯示 X／共 N 筆。 */
function countLabel(shown: number, total: number): string {
  return shown === total ? `共 ${total} 筆` : `顯示 ${shown}／共 ${total} 筆`;
}

function kindColor(kind: InventoryAuditKind): string {
  return kind === "item" ? "teal" : "primary";
}

/** 顯示欄位：null 或空字串以「—」呈現（CSV 仍輸出空；見 spec §6）。 */
function cellText(value: string | null): string {
  return value === null || value === "" ? "—" : value;
}

function displayFilename(filename: string): string {
  return filename === "" ? "（未命名）" : filename;
}

function fileSizeLabel(bytes: number): string {
  return bytes >= 1024 * 1024
    ? `${(bytes / 1024 / 1024).toFixed(1)} MB`
    : `${Math.max(1, Math.round(bytes / 1024))} KB`;
}

function messageOf(cause: unknown): string {
  return cause instanceof Error ? cause.message : String(cause);
}
</script>

<style scoped>
.cell-pre-wrap {
  white-space: pre-wrap;
}
</style>
