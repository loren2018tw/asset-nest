<template>
  <q-page padding>
    <div class="row items-center q-mb-md">
      <div class="text-h6">網段設定</div>
      <q-space />
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
  </q-page>
</template>

<script setup lang="ts">
import type { QTableProps } from "quasar";
import { useQuasar } from "quasar";
import { onMounted, ref } from "vue";

import { deleteSubnet, listSubnets, type SubnetSummary } from "@/api/subnets";
import SubnetFormDialog from "@/components/SubnetFormDialog.vue";

const $q = useQuasar();

const subnets = ref<SubnetSummary[]>([]);
const loading = ref(false);
const dialogOpen = ref(false);
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

function confirmDelete(subnet: SubnetSummary) {
  $q.dialog({
    title: "刪除網段",
    message: `確定要刪除「${subnet.name ?? subnet.cidr}」？`,
    cancel: true,
    persistent: true
  }).onOk(() => {
    void remove(subnet);
  });
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
