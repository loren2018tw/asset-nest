<template>
  <q-page padding>
    <div class="row items-center q-mb-md">
      <div class="text-h6">租約清單</div>
      <div v-if="updatedAt" class="text-caption text-grey-7 q-ml-sm">
        上次更新：{{ formatTime(updatedAt) }}
      </div>
      <q-space />
      <q-btn
        color="primary"
        outline
        icon="refresh"
        label="重新整理"
        :loading="loading"
        @click="load"
      />
    </div>

    <q-banner v-if="loadError" rounded class="bg-negative text-white q-mb-md">
      {{ loadError }}
      <template #action>
        <router-link class="text-white" to="/kea/status">
          前往系統狀態
        </router-link>
      </template>
    </q-banner>

    <div class="row q-col-gutter-sm q-mb-md">
      <div class="col-12 col-md-6">
        <q-input
          v-model="search"
          outlined
          dense
          clearable
          placeholder="搜尋 IP／MAC／Hostname"
        >
          <template #prepend>
            <q-icon name="search" />
          </template>
        </q-input>
      </div>
      <div class="col-12 col-sm-6 col-md-3">
        <q-select
          v-model="stateFilter"
          :options="stateOptions"
          outlined
          dense
          clearable
          emit-value
          map-options
          label="狀態"
        />
      </div>
      <div class="col-12 col-sm-6 col-md-3">
        <q-select
          v-model="reservationFilter"
          :options="reservationOptions"
          outlined
          dense
          clearable
          emit-value
          map-options
          label="保留"
        />
      </div>
    </div>

    <q-table
      :rows="filteredLeases"
      :columns="columns"
      :row-key="rowKey"
      :loading="loading"
      v-model:pagination="pagination"
      :rows-per-page-options="[50]"
      :sort-method="sortLeases"
      binary-state-sort
      :no-data-label="loadError ? '無法取得租約。' : '沒有符合條件的租約。'"
    >
      <template #body-cell-ip_address="props">
        <q-td :props="props" class="lease-mono">
          {{ props.value ?? "—" }}
          <q-badge
            v-if="props.row.is_reservation"
            color="purple"
            class="q-ml-xs"
          >
            保留
            <q-tooltip>{{ reservationHint }}</q-tooltip>
          </q-badge>
        </q-td>
      </template>
      <template #body-cell-hw_address="props">
        <q-td :props="props" class="lease-mono">
          {{ props.value ?? "—" }}
        </q-td>
      </template>
      <template #body-cell-hostname="props">
        <q-td :props="props">{{ props.value ?? "—" }}</q-td>
      </template>
      <template #body-cell-subnet="props">
        <q-td :props="props">{{ subnetLabel(props.row) }}</q-td>
      </template>
      <template #body-cell-expires_at="props">
        <q-td :props="props" :class="{ 'text-grey-6': isExpired(props.row) }">
          {{ formatExpiresAt(props.row.expires_at) }}
        </q-td>
      </template>
      <template #body-cell-state="props">
        <q-td :props="props">
          <q-chip
            v-if="props.row.state !== null"
            dense
            size="sm"
            :color="stateColor(props.row.state)"
            :text-color="stateTextColor(props.row.state)"
          >
            {{ stateLabel(props.row.state) }}
            <q-tooltip>{{ props.row.state }}</q-tooltip>
          </q-chip>
          <span v-else class="text-grey-6">—</span>
        </q-td>
      </template>
      <template #body-cell-actions="props">
        <q-td :props="props">
          <span>
            <q-btn
              dense
              outline
              color="primary"
              label="新增資產及指派 IP"
              :disable="createDisabledReason(props.row) !== null"
              @click="openCreateAsset(props.row)"
            />
            <q-tooltip v-if="createDisabledReason(props.row) !== null">
              {{ createDisabledReason(props.row) }}
            </q-tooltip>
          </span>
        </q-td>
      </template>
    </q-table>

    <asset-form-dialog
      v-model="assetDialogOpen"
      :asset="null"
      :prefill-description="prefillDescription"
      :prefill-interface="prefillInterface"
      @saved="onAssetSaved"
    />
  </q-page>
</template>

<script setup lang="ts">
import type { QTableProps } from "quasar";
import { computed, onMounted, ref } from "vue";

import { listKeaLeases, type KeaLease } from "@/api/kea";
import AssetFormDialog from "@/components/AssetFormDialog.vue";

const leases = ref<KeaLease[]>([]);
const loading = ref(false);
const loadError = ref("");
const updatedAt = ref<Date | null>(null);

const search = ref<string | null>(null);
const stateFilter = ref<string | null>(null);
const reservationFilter = ref<boolean | null>(null);

/** 新增資產對話框（由租約列預填；見 spec「新增資產入口」）。 */
const assetDialogOpen = ref(false);
const prefillDescription = ref("");
const prefillInterface = ref<{ name: string; mac: string } | null>(null);

/** 租約列「保留」標記的 tooltip（受管網段內與 reservation 指派相符）。 */
const reservationHint = "本地保留位址（受管網段內與 reservation 指派相符）";

/** 狀態正規化值（後端已正規化；未知值不在選項內）。 */
const stateOptions = [
  { label: "使用中", value: "default" },
  { label: "已拒絕", value: "declined" },
  { label: "已過期（已回收）", value: "expired-reclaimed" },
  { label: "已釋放", value: "released" },
  { label: "已註冊", value: "registered" }
];

/** 「保留」篩選選項；值對應 `is_reservation`。 */
const reservationOptions = [
  { label: "保留位址", value: true },
  { label: "非保留位址", value: false }
];

/** 客戶端分頁；預設到期時間遞減（越晚到期越上面；排序見 `sortLeases`）。 */
const pagination = ref({
  page: 1,
  rowsPerPage: 50,
  sortBy: "expires_at",
  descending: true
});

const columns: QTableProps["columns"] = [
  {
    name: "ip_address",
    label: "IP",
    field: "ip_address",
    align: "left",
    sortable: true
  },
  { name: "hw_address", label: "MAC", field: "hw_address", align: "left" },
  { name: "hostname", label: "Hostname", field: "hostname", align: "left" },
  {
    name: "subnet",
    label: "網段",
    field: (row: KeaLease) => subnetLabel(row),
    align: "left"
  },
  {
    name: "expires_at",
    label: "到期時間",
    field: "expires_at",
    align: "left",
    sortable: true
  },
  { name: "state", label: "狀態", field: "state", align: "left" },
  { name: "actions", label: "操作", align: "left" }
];

/** 搜尋（IP／MAC／hostname，不分大小寫）＋狀態／保留篩選，皆在客戶端進行。 */
const filteredLeases = computed(() => {
  const needle = (search.value ?? "").trim().toLowerCase();
  const state = stateFilter.value;
  const reservation = reservationFilter.value;

  return leases.value.filter(lease => {
    if (state !== null && lease.state !== state) {
      return false;
    }
    if (reservation !== null && lease.is_reservation !== reservation) {
      return false;
    }
    if (needle === "") {
      return true;
    }
    return [lease.ip_address, lease.hw_address, lease.hostname].some(
      value => value !== null && value.toLowerCase().includes(needle)
    );
  });
});

/** 狀態標籤、顏色與 tooltip 原文（未知值顯示原值）。 */
const stateInfo: Record<string, { label: string; color: string }> = {
  default: { label: "使用中", color: "positive" },
  declined: { label: "已拒絕", color: "warning" },
  "expired-reclaimed": { label: "已過期（已回收）", color: "grey-5" },
  released: { label: "已釋放", color: "blue-grey" },
  registered: { label: "已註冊", color: "teal" }
};

function stateLabel(state: string): string {
  return stateInfo[state]?.label ?? state;
}

function stateColor(state: string): string {
  return stateInfo[state]?.color ?? "grey-7";
}

function stateTextColor(state: string): string {
  return state === "declined" ? "black" : "white";
}

/** 租約以 IP＋MAC 為 key（兩者皆可能為 null）。 */
function rowKey(row: KeaLease): string {
  return `${row.ip_address ?? "?"}#${row.hw_address ?? "?"}`;
}

/** 網段顯示：`CIDR（名稱）`；無本地對應顯示 `Kea #id`；皆缺為「—」。 */
function subnetLabel(lease: KeaLease): string {
  if (lease.subnet_cidr !== null) {
    return lease.subnet_name === null
      ? lease.subnet_cidr
      : `${lease.subnet_cidr}（${lease.subnet_name}）`;
  }
  if (lease.subnet_id !== null) {
    return `Kea #${lease.subnet_id}`;
  }
  return "—";
}

/** 到期時間轉本地時區顯示；未提供原樣（「—」）；無法解析時原樣顯示。 */
function formatExpiresAt(value: string | null): string {
  if (value === null) {
    return "—";
  }
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) {
    return value;
  }
  return date.toLocaleString("zh-TW", {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
    hour12: false
  });
}

/** 已到期（本地現在已過 `expires_at`）→ 淡化顯示。 */
function isExpired(lease: KeaLease): boolean {
  if (lease.expires_at === null) {
    return false;
  }
  const expiresAt = new Date(lease.expires_at).getTime();
  return !Number.isNaN(expiresAt) && expiresAt < Date.now();
}

/** IPv4 轉 32 位元數值；null／非 IPv4 為 null。 */
function ipToNumber(value: string | null): number | null {
  if (value === null) {
    return null;
  }
  const octets = value.split(".");
  if (octets.length !== 4) {
    return null;
  }
  let result = 0;
  for (const octet of octets) {
    const part = Number(octet);
    if (!Number.isInteger(part) || part < 0 || part > 255) {
      return null;
    }
    result = result * 256 + part;
  }
  return result;
}

/** 以八位元組數值比較 IP（非字串序）；無效值固定排在最後。 */
function compareIp(a: string | null, b: string | null): number {
  const left = ipToNumber(a);
  const right = ipToNumber(b);
  if (left === null || right === null) {
    if (left === right) {
      return 0;
    }
    return left === null ? 1 : -1;
  }
  return left - right;
}

/** 到期時間轉時間戳；null／無法解析為 null（排序時固定最後）。 */
function expiresAtTime(value: string | null): number | null {
  if (value === null) {
    return null;
  }
  const time = new Date(value).getTime();
  return Number.isNaN(time) ? null : time;
}

/**
 * 客戶端排序（`:sort-method`）：到期時間為時間戳比較；`null`／無效值固定
 * 排最後（不受升降冪影響；沿用全站「空白固定最後」慣例）；同到期時間以
 * IP 數值升冪決勝。`ip_address` 欄維持數值比較。
 */
function sortLeases(
  rows: readonly KeaLease[],
  sortBy: string,
  descending: boolean
): KeaLease[] {
  const dir = descending ? -1 : 1;

  return [...rows].sort((a, b) => {
    if (sortBy === "expires_at") {
      const left = expiresAtTime(a.expires_at);
      const right = expiresAtTime(b.expires_at);
      if (left === null || right === null) {
        if (left === right) {
          return compareIp(a.ip_address, b.ip_address);
        }
        return left === null ? 1 : -1;
      }
      return left === right
        ? compareIp(a.ip_address, b.ip_address)
        : (left - right) * dir;
    }

    const left = ipToNumber(a.ip_address);
    const right = ipToNumber(b.ip_address);
    if (left === null || right === null) {
      if (left === right) {
        return 0;
      }
      return left === null ? 1 : -1;
    }
    return (left - right) * dir;
  });
}

/** 租約列「新增資產」按鈕停用原因；可用時為 null。 */
function createDisabledReason(lease: KeaLease): string | null {
  if (lease.is_reservation) {
    return "此位址已是保留（已有資產設定）";
  }
  if (lease.subnet_cidr === null) {
    return "租約網段未受管，不提供資產建檔";
  }
  if (lease.ip_address === null) {
    return "租約缺少 IP，不提供資產建檔";
  }
  if (lease.hw_address === null) {
    return "租約缺少 MAC，不提供資產建檔";
  }
  return null;
}

/** 由租約開啟新增資產：預填描述（hostname）與一筆介面（eth0＋租約 MAC）。 */
function openCreateAsset(lease: KeaLease) {
  prefillDescription.value = lease.hostname?.trim() ?? "";
  prefillInterface.value =
    lease.hw_address === null ? null : { name: "eth0", mac: lease.hw_address };
  assetDialogOpen.value = true;
}

/** 資產儲存後：重載租約清單（指派與同步由既有流程處理）。 */
function onAssetSaved() {
  void load();
}

function formatTime(date: Date): string {
  return date.toLocaleTimeString("zh-TW", { hour12: false });
}

/** 載入租約；失敗（400／502 等）顯示 banner 並清空表格。 */
async function load() {
  loading.value = true;
  try {
    leases.value = await listKeaLeases();
    updatedAt.value = new Date();
    loadError.value = "";
  } catch (cause) {
    leases.value = [];
    loadError.value = cause instanceof Error ? cause.message : String(cause);
  } finally {
    loading.value = false;
  }
}

onMounted(() => {
  void load();
});
</script>

<style scoped>
.lease-mono {
  font-family: monospace;
}
</style>
