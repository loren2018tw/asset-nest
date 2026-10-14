<template>
  <q-page padding>
    <div class="row items-center q-mb-md">
      <div class="text-h6">網段設定</div>
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
      <q-btn
        color="primary"
        outline
        icon="sync"
        label="Kea 同步"
        @click="syncOpen = true"
      />
      <q-btn color="primary" icon="add" label="新增網段" @click="openCreate" />
    </div>

    <q-banner rounded class="bg-grey-2 text-grey-8 q-mb-md">
      IPv4 與 IPv6 網段分別建立（雙棧以兩筆表示）；pool 與 Kea subnet-id 僅適用
      IPv4。
    </q-banner>

    <q-table
      :rows="subnets"
      :columns="columns"
      row-key="id"
      :loading="loading"
      :pagination="{ rowsPerPage: 0 }"
      hide-bottom
    >
      <template #body-cell-name="props">
        <q-td :props="props">{{ props.value || "—" }}</q-td>
      </template>
      <template #body-cell-cidr="props">
        <q-td :props="props">
          <router-link
            class="text-primary"
            :to="`/subnets/${props.row.id}/ips`"
          >
            {{ props.value }}
          </router-link>
        </q-td>
      </template>
      <template #body-cell-family="props">
        <q-td :props="props">
          {{ props.value === "ipv4" ? "IPv4" : "IPv6" }}
        </q-td>
      </template>
      <template #body-cell-usage="props">
        <q-td :props="props">
          {{
            props.row.family === "ipv6"
              ? `已登錄 ${props.row.used}`
              : `${props.row.used} / ${props.row.total}`
          }}
        </q-td>
      </template>
      <template #body-cell-conflicts="props">
        <q-td :props="props">
          <q-badge
            v-if="props.row.conflicts > 0"
            color="warning"
            text-color="black"
          >
            {{ props.row.conflicts }}
          </q-badge>
          <span v-else class="text-grey-6">0</span>
        </q-td>
      </template>
      <template #body-cell-actions="props">
        <q-td :props="props" class="text-right">
          <q-btn
            flat
            dense
            round
            icon="format_list_numbered"
            aria-label="IP 清單"
            :to="`/subnets/${props.row.id}/ips`"
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

    <subnet-form-dialog
      v-model="dialogOpen"
      :subnet="editing"
      :subnets="subnets"
      @saved="onSaved"
    />

    <subnet-import-dialog v-model="importOpen" @saved="onSaved" />

    <kea-sync-dialog v-model="syncOpen" />
  </q-page>
</template>

<script setup lang="ts">
import type { QTableProps } from "quasar";
import { useQuasar } from "quasar";
import { onMounted, ref } from "vue";

import {
  deleteSubnet,
  downloadSubnetsCsv,
  fetchSubnet,
  listSubnets,
  type SubnetSummary
} from "@/api/subnets";
import { saveBlob } from "@/api/client";
import KeaSyncDialog from "@/components/KeaSyncDialog.vue";
import SubnetFormDialog from "@/components/SubnetFormDialog.vue";
import SubnetImportDialog from "@/components/SubnetImportDialog.vue";

const $q = useQuasar();

const subnets = ref<SubnetSummary[]>([]);
const loading = ref(false);
const exporting = ref(false);
const dialogOpen = ref(false);
const importOpen = ref(false);
const syncOpen = ref(false);
const editing = ref<SubnetSummary | null>(null);

const columns: QTableProps["columns"] = [
  { name: "name", label: "名稱", field: "name", align: "left" },
  { name: "cidr", label: "CIDR", field: "cidr", align: "left" },
  { name: "family", label: "地址族", field: "family", align: "left" },
  { name: "usage", label: "已用／總數", field: "used", align: "left" },
  { name: "conflicts", label: "衝突數", field: "conflicts", align: "left" },
  { name: "actions", label: "操作", field: "id", align: "right" }
];

function messageOf(cause: unknown): string {
  return cause instanceof Error ? cause.message : String(cause);
}

async function fetchSubnets() {
  loading.value = true;
  try {
    subnets.value = await listSubnets();
  } catch (cause) {
    $q.notify({ type: "negative", message: messageOf(cause) });
  } finally {
    loading.value = false;
  }
}

function openCreate() {
  editing.value = null;
  dialogOpen.value = true;
}

function openEdit(subnet: SubnetSummary) {
  editing.value = subnet;
  dialogOpen.value = true;
}

function onSaved() {
  void fetchSubnets();
}

/** 後端未帶檔名時的預設匯出檔名（本機日期，格式 `網段匯出_YYYYMMDD.csv`）。 */
function exportFilename(date = new Date()): string {
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  return `網段匯出_${year}${month}${day}.csv`;
}

async function exportCsv() {
  exporting.value = true;
  try {
    const { blob, filename } = await downloadSubnetsCsv();
    saveBlob(blob, filename ?? exportFilename());
    $q.notify({ type: "positive", message: "已下載網段匯出檔" });
  } catch (cause) {
    $q.notify({ type: "negative", message: messageOf(cause) });
  } finally {
    exporting.value = false;
  }
}

/**
 * 刪除確認：受管網段（Kea subnet-id 有值）加註 Kea 端不會自動清理
 * （見 ADR-0027）；詳情讀取失敗則不進入刪除流程。
 */
async function confirmDelete(subnet: SubnetSummary) {
  let keaNote = "";
  try {
    const detail = await fetchSubnet(subnet.id);
    if (detail.kea_subnet_id !== null) {
      keaNote =
        `<br><br><span class="text-warning">此網段受 Kea 管理（subnet-id ${detail.kea_subnet_id}）：` +
        `Kea 端網段與位址池不會被移除，需自行於 Kea 清理。</span>`;
    }
  } catch (cause) {
    $q.notify({ type: "negative", message: messageOf(cause) });
    return;
  }

  $q.dialog({
    title: "刪除網段",
    message:
      `確定要刪除「${escapeHtml(subnet.name ?? subnet.cidr)}」？` + keaNote,
    html: true,
    cancel: true,
    persistent: true
  }).onOk(() => {
    void remove(subnet);
  });
}

/** 對話框以 HTML 呈現提示資訊；網段名稱為使用者輸入，需轉義。 */
function escapeHtml(value: string): string {
  return value
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#39;");
}

async function remove(subnet: SubnetSummary) {
  try {
    await deleteSubnet(subnet.id);
    $q.notify({ type: "positive", message: "已刪除網段" });
    await fetchSubnets();
  } catch (cause) {
    $q.notify({ type: "negative", message: messageOf(cause) });
  }
}

onMounted(() => {
  void fetchSubnets();
});
</script>
