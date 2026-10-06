<template>
  <q-dialog v-model="open" persistent>
    <q-card class="import-dialog column no-wrap">
      <q-card-section class="q-pb-sm">
        <div class="row items-center">
          <div class="text-h6">匯入網段</div>
          <q-space />
          <div class="text-caption text-grey-7">
            步驟 {{ stepNumber }}／3：{{ stepLabel }}
          </div>
        </div>
        <div class="row q-gutter-xs q-mt-sm">
          <q-chip
            v-for="item in stepChips"
            :key="item.step"
            dense
            :color="step === item.step ? 'primary' : 'grey-3'"
            :text-color="step === item.step ? 'white' : 'black'"
          >
            {{ item.label }}
          </q-chip>
        </div>
      </q-card-section>

      <q-separator />

      <q-card-section class="col q-pt-md" style="overflow: auto">
        <!-- ① 選檔 -->
        <template v-if="step === 'choose'">
          <q-banner
            v-if="errorMessage !== ''"
            dense
            rounded
            class="bg-negative text-white q-mb-md"
          >
            {{ errorMessage }}
          </q-banner>

          <div class="row q-col-gutter-md">
            <div class="col-12 col-md-7">
              <q-uploader
                :key="uploaderKey"
                class="full-width"
                label="選擇 CSV 檔（.xlsx 不支援）"
                accept=".csv"
                :auto-upload="false"
                :multiple="false"
                flat
                bordered
                hide-upload-btn
                @added="onFileAdded"
                @removed="onFileRemoved"
                @rejected="onFileRejected"
              />
              <div v-if="selectedFile !== null" class="text-caption q-mt-sm">
                已選擇：{{ selectedFile.name }}（{{ fileSizeLabel }}）
              </div>
            </div>

            <div class="col-12 col-md-5">
              <q-list dense class="text-body2">
                <q-item>
                  <q-item-section avatar>
                    <q-icon name="translate" color="primary" />
                  </q-item-section>
                  <q-item-section>
                    編碼：UTF-8（含 BOM）或
                    Big5（CP950）皆可，系統自動偵測並於預覽顯示
                  </q-item-section>
                </q-item>
                <q-item>
                  <q-item-section avatar>
                    <q-icon name="view_column" color="primary" />
                  </q-item-section>
                  <q-item-section>
                    第一列為標題，共 7 欄（名稱／CIDR／Gateway／Kea
                    subnet-id／位址池／排除範圍／備註）；欄位順序不拘，未知欄位會忽略
                  </q-item-section>
                </q-item>
                <q-item>
                  <q-item-section avatar>
                    <q-icon name="settings_ethernet" color="primary" />
                  </q-item-section>
                  <q-item-section>
                    Gateway 須在該 CIDR 內；Kea subnet-id
                    為正整數、全系統唯一，僅 IPv4
                  </q-item-section>
                </q-item>
                <q-item>
                  <q-item-section avatar>
                    <q-icon name="dns" color="primary" />
                  </q-item-section>
                  <q-item-section>
                    位址池以「|」分隔多段，每段「起點-終點」（例：
                    10.0.0.100-10.0.0.200|10.0.0.250-10.0.0.253），僅 IPv4
                  </q-item-section>
                </q-item>
                <q-item>
                  <q-item-section avatar>
                    <q-icon name="block" color="primary" />
                  </q-item-section>
                  <q-item-section>
                    排除範圍（選填）亦以「|」分隔多段，每段「起-迄#用途說明」
                    （說明選填、以第一個 # 分隔），僅 IPv4
                  </q-item-section>
                </q-item>
                <q-item>
                  <q-item-section avatar>
                    <q-icon name="playlist_add" color="primary" />
                  </q-item-section>
                  <q-item-section>
                    只新增：CIDR 不得與既有或檔內其他列重複、重疊（含嵌套）
                  </q-item-section>
                </q-item>
                <q-item>
                  <q-item-section avatar>
                    <q-icon name="rule" color="primary" />
                  </q-item-section>
                  <q-item-section>
                    全有全無：任一列有結構錯誤，整批不匯入
                  </q-item-section>
                </q-item>
              </q-list>
            </div>
          </div>
        </template>

        <!-- ② 預覽 -->
        <template v-else-if="step === 'preview'">
          <div class="row items-center q-gutter-x-sm q-mb-sm">
            <q-chip dense square color="grey-3" text-color="black">
              總筆數 {{ summary.total }}
            </q-chip>
            <q-chip dense square color="positive" text-color="white">
              OK {{ summary.ok }}
            </q-chip>
            <q-chip dense square color="warning" text-color="black">
              警示 {{ summary.warnings }}
            </q-chip>
            <q-chip dense square color="negative" text-color="white">
              錯誤 {{ summary.errors }}
            </q-chip>
            <q-chip
              dense
              square
              color="primary"
              text-color="white"
              icon="translate"
            >
              偵測編碼：{{ encodingLabel }}
            </q-chip>
            <q-space />
            <q-toggle v-model="onlyIssues" dense label="只看問題列" />
          </div>

          <q-banner
            v-if="ignoredHeaders.length > 0"
            dense
            rounded
            class="bg-info text-white q-mb-sm"
          >
            已忽略未知欄位：{{ ignoredHeaders.join("、") }}
          </q-banner>

          <q-banner
            v-if="summary.errors > 0"
            dense
            rounded
            class="bg-negative text-white q-mb-sm"
          >
            有
            {{ summary.errors }}
            列錯誤；依「全有全無」規則整批不會匯入，請下載問題列報告修正後重新上傳整份檔案。
          </q-banner>

          <q-table
            :rows="displayRows"
            :columns="columns"
            row-key="row_number"
            dense
            flat
            bordered
            no-wrap
            hide-bottom
            virtual-scroll
            :pagination="{ rowsPerPage: 0 }"
            class="import-table"
            style="height: 52vh; min-height: 320px"
          >
            <template #body-cell-status="props">
              <q-td :props="props" class="text-center">
                <q-badge
                  :color="statusColor(props.row.status)"
                  :text-color="statusTextColor(props.row.status)"
                >
                  {{ statusLabel(props.row.status) }}
                </q-badge>
              </q-td>
            </template>
            <template #body-cell-reason="props">
              <q-td :props="props" :title="props.value">
                {{ props.value }}
              </q-td>
            </template>
            <template #no-data>
              <div class="full-width row flex-center text-grey-7 q-pa-md">
                沒有符合的列
              </div>
            </template>
          </q-table>
        </template>

        <!-- ③ 結果 -->
        <template v-else>
          <q-banner dense rounded class="bg-positive text-white q-mb-md">
            <template #avatar>
              <q-icon name="check_circle" />
            </template>
            匯入完成
          </q-banner>

          <div class="row q-gutter-sm q-mb-md">
            <q-chip dense square color="positive" text-color="white" icon="lan">
              網段 {{ created?.subnets ?? 0 }}
            </q-chip>
            <q-chip
              dense
              square
              :color="resultWarningCount > 0 ? 'warning' : 'grey-3'"
              text-color="black"
            >
              警示 {{ resultWarningCount }}
            </q-chip>
          </div>

          <q-banner
            v-if="resultWarningCount > 0"
            dense
            rounded
            class="bg-warning text-black q-mb-md"
          >
            有
            {{ resultWarningCount }}
            列警示（不阻擋匯入，已照常建立）；可下載問題列報告核對。
          </q-banner>
        </template>
      </q-card-section>

      <q-separator />

      <q-card-actions class="q-pa-md">
        <template v-if="step === 'choose'">
          <q-btn flat label="取消" @click="close" />
          <q-space />
          <q-btn
            color="primary"
            icon="preview"
            label="預覽"
            :loading="previewing"
            :disable="selectedFile === null"
            @click="startPreview"
          />
        </template>

        <template v-else-if="step === 'preview'">
          <q-btn
            flat
            label="上一步"
            :disable="importing"
            @click="backToChoose"
          />
          <q-btn
            flat
            label="重新選檔"
            :disable="importing"
            @click="reselectFile"
          />
          <q-space />
          <q-btn
            v-if="issueRowCount > 0"
            flat
            color="primary"
            icon="download"
            label="下載問題列報告"
            @click="downloadIssueReport"
          />
          <div
            v-if="summary.errors > 0"
            class="text-caption text-negative q-mr-sm"
          >
            有 {{ summary.errors }} 列錯誤，整批不匯入
          </div>
          <q-btn
            color="primary"
            icon="upload"
            :label="importLabel"
            :loading="importing"
            :disable="!canImport"
            @click="confirmImport"
          />
        </template>

        <template v-else>
          <q-btn
            v-if="resultIssueCount > 0"
            flat
            color="primary"
            icon="download"
            label="下載問題列報告"
            @click="downloadResultIssueReport"
          />
          <q-space />
          <q-btn color="primary" label="完成" @click="finish" />
        </template>
      </q-card-actions>
    </q-card>
  </q-dialog>
</template>

<script setup lang="ts">
import type { QRejectedEntry, QTableProps } from "quasar";
import { useQuasar } from "quasar";
import { computed, ref, watch } from "vue";

import {
  importSubnets,
  type SubnetImportReport,
  type SubnetImportRow,
  type SubnetImportRowStatus,
  type SubnetImportSummary
} from "@/api/subnets";
import { downloadImportIssueReport, issueRowsOf } from "@/utils/subnetImport";

const props = defineProps<{
  modelValue: boolean;
}>();

const emit = defineEmits<{
  "update:modelValue": [value: boolean];
  saved: [];
}>();

const $q = useQuasar();

const open = computed({
  get: () => props.modelValue,
  set: value => emit("update:modelValue", value)
});

/** 對話框三步（見 spec §6）。 */
type Step = "choose" | "preview" | "result";

const step = ref<Step>("choose");
const uploaderKey = ref(0);
const selectedFile = ref<File | null>(null);
const previewing = ref(false);
const importing = ref(false);
const errorMessage = ref("");
/** 預覽中（或資料變動後更新）的回應；步驟②的資料來源。 */
const report = ref<SubnetImportReport | null>(null);
/** 正式匯入成功後的回應；步驟③的資料來源。 */
const resultReport = ref<SubnetImportReport | null>(null);
const onlyIssues = ref(false);

const stepChips: { step: Step; label: string }[] = [
  { step: "choose", label: "1 選檔" },
  { step: "preview", label: "2 預覽" },
  { step: "result", label: "3 結果" }
];

const stepNumber = computed(() => {
  if (step.value === "choose") {
    return 1;
  }
  return step.value === "preview" ? 2 : 3;
});

const stepLabel = computed(() => {
  if (step.value === "choose") {
    return "選檔";
  }
  return step.value === "preview" ? "預覽" : "結果";
});

const summary = computed<SubnetImportSummary>(
  () => report.value?.summary ?? { total: 0, ok: 0, warnings: 0, errors: 0 }
);

const encodingLabel = computed(() => {
  const encoding = report.value?.encoding;
  if (encoding === undefined) {
    return "—";
  }
  return encoding === "big5" ? "Big5（CP950）" : "UTF-8";
});

const ignoredHeaders = computed(() => report.value?.ignored_headers ?? []);

const issueRowCount = computed(() =>
  report.value === null ? 0 : issueRowsOf(report.value).length
);

const displayRows = computed<SubnetImportRow[]>(() => {
  const current = report.value;
  if (current === null) {
    return [];
  }
  return onlyIssues.value
    ? current.rows.filter(row => row.issues.length > 0)
    : current.rows;
});

/** 匯入筆數＝總數－錯誤數（見 spec §3）。 */
const importCount = computed(() => summary.value.total - summary.value.errors);

const importLabel = computed(() => `匯入 ${importCount.value} 筆`);

const canImport = computed(
  () => report.value !== null && summary.value.errors === 0
);

const created = computed(() => resultReport.value?.created ?? null);

const resultWarningCount = computed(
  () => resultReport.value?.summary.warnings ?? 0
);

const resultIssueCount = computed(() =>
  resultReport.value === null ? 0 : issueRowsOf(resultReport.value).length
);

const fileSizeLabel = computed(() => {
  const file = selectedFile.value;
  if (file === null) {
    return "";
  }
  const kib = file.size / 1024;
  return kib >= 1024
    ? `${(kib / 1024).toFixed(1)} MB`
    : `${Math.max(1, Math.round(kib))} KB`;
});

const columns: QTableProps["columns"] = [
  { name: "row_number", label: "列號", field: "row_number", align: "right" },
  {
    name: "name",
    label: "名稱",
    field: (row: SubnetImportRow) => cellText(row.data.name),
    align: "left"
  },
  {
    name: "cidr",
    label: "CIDR",
    field: (row: SubnetImportRow) => cellText(row.data.cidr),
    align: "left"
  },
  {
    name: "gateway",
    label: "Gateway",
    field: (row: SubnetImportRow) => cellText(row.data.gateway),
    align: "left"
  },
  {
    name: "kea_subnet_id",
    label: "Kea subnet-id",
    field: (row: SubnetImportRow) => cellText(row.data.kea_subnet_id),
    align: "right"
  },
  {
    name: "pools",
    label: "位址池",
    field: (row: SubnetImportRow) =>
      row.data.pools.length === 0 ? "—" : row.data.pools.join("|"),
    align: "left"
  },
  {
    name: "exclusions",
    label: "排除範圍",
    field: (row: SubnetImportRow) =>
      row.data.exclusions.length === 0 ? "—" : row.data.exclusions.join("|"),
    align: "left"
  },
  {
    name: "note",
    label: "備註",
    field: (row: SubnetImportRow) => cellText(row.data.note),
    align: "left"
  },
  { name: "status", label: "狀態", field: "status", align: "center" },
  {
    name: "reason",
    label: "原因",
    field: (row: SubnetImportRow) => reasonOf(row),
    align: "left"
  }
];

watch(
  () => props.modelValue,
  value => {
    if (value) {
      prepare();
    }
  }
);

/** 開啟時重置回步驟①。 */
function prepare() {
  step.value = "choose";
  uploaderKey.value += 1;
  selectedFile.value = null;
  previewing.value = false;
  importing.value = false;
  errorMessage.value = "";
  report.value = null;
  resultReport.value = null;
  onlyIssues.value = false;
}

/** QUploader 的檔案即原生 `File`（見 Quasar 實作）。 */
function onFileAdded(files: readonly File[]) {
  const added = files[0];
  if (added !== undefined) {
    selectedFile.value = added;
  }
}

function onFileRemoved(files: readonly File[]) {
  if (selectedFile.value !== null && files.includes(selectedFile.value)) {
    selectedFile.value = null;
  }
}

function onFileRejected(entries: QRejectedEntry[]) {
  const rejected = entries[0];
  if (rejected === undefined) {
    return;
  }
  const message =
    rejected.failedPropValidation === "accept"
      ? `僅支援 .csv 檔（${rejected.file.name}）；.xlsx 請先另存為 CSV`
      : `檔案未通過檢查（${rejected.file.name}）`;
  $q.notify({ type: "warning", message, timeout: 6000 });
}

/** 步驟①→②：`dry_run=true` 預覽（不寫入任何資料）。 */
async function startPreview() {
  const file = selectedFile.value;
  if (file === null) {
    return;
  }
  previewing.value = true;
  errorMessage.value = "";
  try {
    report.value = await importSubnets(file, true);
    onlyIssues.value = false;
    step.value = "preview";
  } catch (cause) {
    errorMessage.value = messageOf(cause);
  } finally {
    previewing.value = false;
  }
}

function backToChoose() {
  step.value = "choose";
  errorMessage.value = "";
}

/** 重新選檔：清空佇列（強制重建 uploader）並回步驟①。 */
function reselectFile() {
  selectedFile.value = null;
  uploaderKey.value += 1;
  backToChoose();
}

/** 有錯誤時停用；僅有警示時先跳確認（見 spec §6）。 */
function confirmImport() {
  const current = report.value;
  if (current === null || current.summary.errors > 0) {
    return;
  }

  if (current.summary.warnings > 0) {
    $q.dialog({
      title: "確認匯入",
      message:
        `有 ${current.summary.warnings} 列警示（不阻擋匯入）；` +
        `將建立 ${importCount.value} 筆網段，確定要繼續？`,
      cancel: true,
      persistent: true
    }).onOk(() => {
      void commitImport();
    });
    return;
  }

  void commitImport();
}

/** 步驟②→③：`dry_run=false` 正式匯入。 */
async function commitImport() {
  const file = selectedFile.value;
  if (file === null) {
    return;
  }
  importing.value = true;
  try {
    const result = await importSubnets(file, false);
    if (result.committed) {
      resultReport.value = result;
      step.value = "result";
      return;
    }

    // 預覽後資料已變動：以正式匯入的重驗結果更新預覽（見 spec §3）
    report.value = result;
    onlyIssues.value = false;
    $q.notify({
      type: "warning",
      message: "資料已變動，未匯入，請確認後再試",
      timeout: 8000
    });
  } catch (cause) {
    $q.notify({ type: "negative", message: messageOf(cause), timeout: 8000 });
  } finally {
    importing.value = false;
  }
}

function downloadIssueReport() {
  if (report.value !== null) {
    downloadImportIssueReport(report.value);
  }
}

function downloadResultIssueReport() {
  if (resultReport.value !== null) {
    downloadImportIssueReport(resultReport.value);
  }
}

function close() {
  open.value = false;
}

/** 步驟③「完成」：關閉並通知外部重載網段列表。 */
function finish() {
  emit("saved");
  close();
}

function statusColor(status: SubnetImportRowStatus): string {
  if (status === "ok") {
    return "positive";
  }
  return status === "warning" ? "warning" : "negative";
}

function statusTextColor(status: SubnetImportRowStatus): string {
  return status === "warning" ? "black" : "white";
}

function statusLabel(status: SubnetImportRowStatus): string {
  if (status === "ok") {
    return "OK";
  }
  return status === "warning" ? "警示" : "錯誤";
}

/** 原因＝issues 訊息以「；」串接（見 spec §3）。 */
function reasonOf(row: SubnetImportRow): string {
  return row.issues.map(issue => issue.message).join("；");
}

function cellText(value: string | number | null): string {
  return value === null || value === "" ? "—" : String(value);
}

function messageOf(cause: unknown): string {
  return cause instanceof Error ? cause.message : String(cause);
}
</script>

<style scoped>
.import-dialog {
  width: 1400px;
  max-width: 95vw;
  height: 92vh;
}
</style>
