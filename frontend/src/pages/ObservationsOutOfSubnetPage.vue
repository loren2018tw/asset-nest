<template>
  <q-page padding>
    <div class="row items-center q-mb-md">
      <div class="text-h6">網段外觀測</div>
      <q-space />
      <q-btn
        color="primary"
        outline
        icon="refresh"
        label="重新整理"
        :loading="loading"
        @click="fetchObservations"
      />
    </div>

    <q-table
      :rows="observations"
      :columns="columns"
      :row-key="rowKey"
      :loading="loading"
      :pagination="{ rowsPerPage: 0 }"
      hide-bottom
    >
      <template #body-cell-address="props">
        <q-td :props="props">
          <span class="mono-text">{{ props.value }}</span>
        </q-td>
      </template>
      <template #body-cell-mac="props">
        <q-td :props="props">
          <template v-if="props.row.mac">
            <q-btn
              flat
              dense
              no-caps
              color="primary"
              class="mono-text q-px-none"
              :aria-label="`MAC 觀測歷史 ${props.row.mac}`"
              :label="props.row.mac"
              @click="openHistory(props.row.mac)"
            >
              <q-tooltip>觀測歷史</q-tooltip>
            </q-btn>
            <q-badge
              v-if="!props.row.known"
              color="grey-7"
              class="q-ml-sm"
              label="未登錄"
            />
            <div v-else-if="props.row.asset" class="text-caption text-grey-7">
              {{
                assetLabel(
                  props.row.asset.property_no,
                  props.row.asset.description
                )
              }}
              ｜ {{ props.row.asset.location }}
            </div>
          </template>
          <span v-else class="text-grey-6">—</span>
        </q-td>
      </template>
      <!-- 首次看到／最後看到（見票 02）：相對時間，tooltip 為精確時間 -->
      <template #body-cell-first_seen="props">
        <q-td :props="props">
          {{ relativeTime(props.row.first_seen_at) }}
          <q-tooltip>{{ exactTime(props.row.first_seen_at) }}</q-tooltip>
        </q-td>
      </template>
      <template #body-cell-last_seen="props">
        <q-td :props="props">
          {{ relativeTime(props.row.last_seen_at) }}
          <q-tooltip>{{ exactTime(props.row.last_seen_at) }}</q-tooltip>
        </q-td>
      </template>
      <template #body-cell-source="props">
        <q-td :props="props">{{ sourceLabel(props.row.source) }}</q-td>
      </template>
      <template #body-cell-subnet="props">
        <q-td :props="props">
          {{ props.row.subnet_name ?? "—" }} ｜
          <span class="mono-text">{{ props.row.subnet_cidr }}</span>
        </q-td>
      </template>
      <!-- 空狀態（見票 02）：說明資料只在探索掃描被動監聽時產生 -->
      <template #no-data>
        <div class="full-width q-px-md q-py-md text-grey-7">
          <div class="text-subtitle2 q-mb-xs">尚無網段外觀測資料</div>
          <ul class="q-my-none q-pl-lg">
            <li>
              只於<strong>探索掃描</strong>時，在被探測網段的介面上被動監聽
              ARP；快速掃描不監聽。
            </li>
            <li>
              需要 raw 模式（CAP_NET_RAW）；unprivileged
              降級模式無法被動監聽，不會有資料。
            </li>
            <li>
              監聽窗長以環境變數
              <span class="mono-text">OBSERVATION_PASSIVE_WINDOW_SECS</span>
              設定（預設 60 秒、0＝停用）。
            </li>
            <li>命中率取決於設備活動：閒置設備可能不出現。</li>
          </ul>
        </div>
      </template>
    </q-table>

    <observation-history-dialog v-model="historyOpen" :mac="historyMac" />
  </q-page>
</template>

<script setup lang="ts">
import type { QTableProps } from "quasar";
import { useQuasar } from "quasar";
import { onMounted, ref } from "vue";

import {
  listOutOfSubnetObservations,
  type OutOfSubnetObservation
} from "@/api/observations";
import ObservationHistoryDialog from "@/components/ObservationHistoryDialog.vue";
import { assetLabel } from "@/utils/assetLabel";
import { sourceLabel } from "@/utils/observationSource";
import { relativeTime } from "@/utils/relativeTime";

const $q = useQuasar();

const observations = ref<OutOfSubnetObservation[]>([]);
const loading = ref(false);
/** 觀測歷史對話框（MAC 模式；mac 為 null 的列不開啟）。 */
const historyOpen = ref(false);
const historyMac = ref("");

const columns: QTableProps["columns"] = [
  { name: "address", label: "IP", field: "address", align: "left" },
  { name: "mac", label: "MAC", field: "mac", align: "left" },
  {
    name: "first_seen",
    label: "首次看到",
    field: "first_seen_at",
    align: "left"
  },
  {
    name: "last_seen",
    label: "最後看到",
    field: "last_seen_at",
    align: "left"
  },
  { name: "source", label: "來源", field: "source", align: "left" },
  { name: "subnet", label: "觀測網段", field: "subnet_cidr", align: "left" }
];

/** 同一網段外位址可歸屬多個探測網段（ADR-0017），列鍵須含網段。 */
function rowKey(row: OutOfSubnetObservation): string {
  return `${row.subnet_id}-${row.address}`;
}

function messageOf(cause: unknown): string {
  return cause instanceof Error ? cause.message : String(cause);
}

async function fetchObservations() {
  loading.value = true;
  try {
    observations.value = (await listOutOfSubnetObservations()).items;
  } catch (cause) {
    $q.notify({ type: "negative", message: messageOf(cause) });
  } finally {
    loading.value = false;
  }
}

function openHistory(mac: string) {
  historyMac.value = mac;
  historyOpen.value = true;
}

/** 精確時間：顯示瀏覽器本地時間；解析失敗原樣顯示。 */
function exactTime(value: string): string {
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? value : date.toLocaleString();
}

onMounted(() => {
  void fetchObservations();
});
</script>

<style scoped>
.mono-text {
  font-family: monospace;
}
</style>
