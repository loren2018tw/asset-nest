<template>
  <q-page padding>
    <div class="row items-center q-mb-md">
      <div class="text-h6">系統狀態</div>
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
    </q-banner>

    <div class="row q-col-gutter-md">
      <div class="col-12">
        <q-card flat bordered>
          <q-card-section>
            <div class="text-subtitle1 q-mb-sm">asset-nest 服務</div>
            <template v-if="health">
              <div class="row q-col-gutter-md">
                <div class="col-6 col-md-3">
                  <div class="text-caption text-grey-7">狀態</div>
                  <div>{{ health.status }}</div>
                </div>
                <div class="col-6 col-md-3">
                  <div class="text-caption text-grey-7">服務</div>
                  <div>{{ health.service }}</div>
                </div>
                <div class="col-6 col-md-3">
                  <div class="text-caption text-grey-7">版本</div>
                  <div>{{ health.version }}</div>
                </div>
                <div class="col-6 col-md-3">
                  <div class="text-caption text-grey-7">資料庫</div>
                  <div>{{ health.database }}</div>
                </div>
              </div>
            </template>
            <q-banner
              v-else-if="healthError"
              dense
              rounded
              class="bg-negative text-white"
            >
              無法連線後端：{{ healthError }}
            </q-banner>
            <div v-else class="text-grey-7">載入中…</div>
          </q-card-section>
        </q-card>
      </div>

      <div class="col-12 col-md-6">
        <q-card flat bordered class="fit">
          <q-card-section>
            <div class="text-subtitle1 q-mb-sm">連線</div>
            <template v-if="status">
              <template v-if="!status.configured">
                <q-chip dense color="grey-7" text-color="white">未設定</q-chip>
                <div class="text-grey-7 q-mt-sm">
                  未設定 Kea 連線（KEA_API_URL）
                </div>
              </template>
              <template v-else>
                <q-chip
                  dense
                  :color="status.reachable ? 'positive' : 'negative'"
                  text-color="white"
                >
                  {{ status.reachable ? "可達" : "無法連線" }}
                </q-chip>
                <div v-if="status.url" class="text-caption q-mt-sm">
                  連線目標：{{ status.url }}
                </div>
                <div
                  v-if="!status.reachable && status.errors?.version"
                  class="text-caption text-grey-7 q-mt-xs"
                >
                  {{ status.errors.version }}
                </div>
              </template>
            </template>
            <div v-else class="text-grey-7">無法取得</div>
          </q-card-section>
        </q-card>
      </div>

      <div class="col-12 col-md-6">
        <q-card flat bordered class="fit">
          <q-card-section>
            <div class="text-subtitle1 q-mb-sm">版本</div>
            <template v-if="versionText !== null">
              <div>{{ versionText }}</div>
            </template>
            <template v-else>
              <div class="text-grey-7">無法取得</div>
              <div
                v-if="status?.errors?.version"
                class="text-caption text-grey-7"
              >
                {{ status.errors.version }}
              </div>
            </template>
          </q-card-section>
        </q-card>
      </div>

      <div class="col-12 col-md-6">
        <q-card flat bordered class="fit">
          <q-card-section>
            <div class="text-subtitle1 q-mb-sm">監聽介面</div>
            <template v-if="status?.interfaces != null">
              <div v-if="status.interfaces.length === 0" class="text-grey-7">
                未監聽任何介面（不主動服務 DHCP；安裝預設）
              </div>
              <template v-else>
                <q-chip
                  v-for="name in status.interfaces"
                  :key="name"
                  dense
                  color="primary"
                  text-color="white"
                >
                  {{ name }}
                </q-chip>
              </template>
            </template>
            <template v-else>
              <div class="text-grey-7">無法取得</div>
              <div
                v-if="status?.errors?.config"
                class="text-caption text-grey-7"
              >
                {{ status.errors.config }}
              </div>
            </template>
          </q-card-section>
        </q-card>
      </div>

      <div class="col-12 col-md-6">
        <q-card flat bordered class="fit">
          <q-card-section>
            <div class="text-subtitle1 q-mb-sm">運行資訊</div>
            <template v-if="status?.runtime">
              <div class="row q-col-gutter-md">
                <div class="col-6">
                  <div class="text-caption text-grey-7">PID</div>
                  <div>{{ status.runtime.pid ?? "—" }}</div>
                </div>
                <div class="col-6">
                  <div class="text-caption text-grey-7">運行時間</div>
                  <div>{{ uptimeText }}</div>
                </div>
                <div class="col-6">
                  <div class="text-caption text-grey-7">上次設定重載</div>
                  <div>{{ reloadText }}</div>
                </div>
                <div v-if="socketText !== null" class="col-6">
                  <div class="text-caption text-grey-7">socket 狀態</div>
                  <div>{{ socketText }}</div>
                </div>
              </div>
            </template>
            <template v-else>
              <div class="text-grey-7">無法取得</div>
              <div
                v-if="status?.errors?.status"
                class="text-caption text-grey-7"
              >
                {{ status.errors.status }}
              </div>
            </template>
          </q-card-section>
        </q-card>
      </div>

      <div class="col-12">
        <q-card flat bordered>
          <q-card-section>
            <div class="text-subtitle1 q-mb-sm">DHCPv4 摘要</div>
            <template v-if="status?.dhcp4">
              <div class="row q-col-gutter-md">
                <div class="col-6 col-md-4">
                  <div class="text-caption text-grey-7">Kea 網段數</div>
                  <div>
                    {{ status.dhcp4.subnet_count }}
                    <q-chip
                      v-if="subnetCountMismatch"
                      dense
                      size="sm"
                      color="warning"
                      text-color="black"
                      class="q-ml-sm"
                    >
                      不一致
                      <q-tooltip>
                        Kea 的網段數與本地受管網段數不一致，請確認 kea_subnet_id
                        設定
                      </q-tooltip>
                    </q-chip>
                  </div>
                </div>
                <div class="col-6 col-md-4">
                  <div class="text-caption text-grey-7">本地受管網段數</div>
                  <div>{{ status.dhcp4.managed_subnet_count }}</div>
                </div>
                <div class="col-6 col-md-4">
                  <div class="text-caption text-grey-7">租約庫類型</div>
                  <div>{{ status.dhcp4.lease_backend ?? "—" }}</div>
                </div>
              </div>
            </template>
            <template v-else>
              <div class="text-grey-7">無法取得</div>
              <div
                v-if="status?.errors?.config"
                class="text-caption text-grey-7"
              >
                {{ status.errors.config }}
              </div>
            </template>
          </q-card-section>
        </q-card>
      </div>

      <div class="col-12">
        <q-card flat bordered>
          <q-card-section>
            <div class="row items-baseline">
              <div class="text-subtitle1 q-mb-sm">觀測代理</div>
              <div
                v-if="agents"
                class="text-caption text-grey-7 q-ml-sm q-mb-sm"
              >
                在線門檻：{{ humanizeDuration(staleSecs) }}內有回報
              </div>
            </div>
            <q-table
              v-if="agents"
              :rows="agents"
              :columns="agentColumns"
              row-key="instance_id"
              :loading="loading"
              :pagination="{ rowsPerPage: 0 }"
              hide-bottom
            >
              <template #body-cell-name="props">
                <q-td :props="props">
                  <div>{{ props.row.name }}</div>
                  <div class="text-caption text-grey-7 mono-text">
                    {{ props.row.instance_id }}
                  </div>
                </q-td>
              </template>
              <template #body-cell-source_ip="props">
                <q-td :props="props">
                  <span class="mono-text">{{ props.row.source_ip }}</span>
                </q-td>
              </template>
              <template #body-cell-subnet="props">
                <q-td :props="props">
                  <span class="mono-text">{{ props.row.subnet_cidr }}</span>
                  <div
                    v-if="props.row.subnet_name"
                    class="text-caption text-grey-7"
                  >
                    {{ props.row.subnet_name }}
                  </div>
                </q-td>
              </template>
              <template #body-cell-last_report="props">
                <q-td :props="props">
                  {{ relativeTime(props.row.last_report_at) }}
                  <q-tooltip>
                    {{ exactTime(props.row.last_report_at) }}
                  </q-tooltip>
                </q-td>
              </template>
              <template #body-cell-status="props">
                <q-td :props="props">
                  <q-chip
                    dense
                    :color="agentStatus(props.row).color"
                    :text-color="agentStatus(props.row).textColor"
                  >
                    {{ agentStatus(props.row).label }}
                  </q-chip>
                  <q-tooltip v-if="agentStatus(props.row).hint">
                    {{ agentStatus(props.row).hint }}
                  </q-tooltip>
                </q-td>
              </template>
              <!-- 空狀態（見票 01）：說明代理回報與狀態語意 -->
              <template #no-data>
                <div class="full-width q-px-md q-py-md text-grey-7">
                  <div class="text-subtitle2 q-mb-xs">尚無觀測代理回報</div>
                  <ul class="q-my-none q-pl-lg">
                    <li>
                      在目標網段安裝觀測代理後，代理會定期回報心跳並顯示於此。
                    </li>
                    <li>
                      涵蓋網段需與受管網段 CIDR 精確相符，否則標示「未對應」。
                    </li>
                    <li>
                      最後回報超過在線門檻
                      <span class="mono-text">AGENT_STALE_SECS</span>
                      即標示「離線」；代理資料保留、不清除。
                    </li>
                  </ul>
                </div>
              </template>
            </q-table>
            <q-banner
              v-else-if="agentsError"
              dense
              rounded
              class="bg-negative text-white"
            >
              無法取得觀測代理：{{ agentsError }}
            </q-banner>
            <div v-else class="text-grey-7">載入中…</div>
          </q-card-section>
        </q-card>
      </div>

      <div class="col-12">
        <q-card flat bordered>
          <q-card-section>
            <div class="text-subtitle1 q-mb-sm">被拒回報</div>
            <q-table
              v-if="authFailures"
              :rows="authFailures"
              :columns="authFailureColumns"
              row-key="source_ip"
              :loading="loading"
              :pagination="{ rowsPerPage: 0 }"
              hide-bottom
            >
              <template #body-cell-source_ip="props">
                <q-td :props="props">
                  <span class="mono-text">{{ props.row.source_ip }}</span>
                </q-td>
              </template>
              <template #body-cell-claimed="props">
                <q-td :props="props">
                  {{ props.row.claimed_name ?? "—" }}
                  <span class="text-grey-7">／</span>
                  {{ props.row.claimed_version ?? "—" }}
                </q-td>
              </template>
              <template #body-cell-last_attempt="props">
                <q-td :props="props">
                  {{ relativeTime(props.row.last_attempt_at) }}
                  <q-tooltip>
                    {{ exactTime(props.row.last_attempt_at) }}
                  </q-tooltip>
                </q-td>
              </template>
              <!-- 空狀態（見票 01）：說明被拒回報的產生條件 -->
              <template #no-data>
                <div class="full-width q-px-md q-py-md text-grey-7">
                  <div class="text-subtitle2 q-mb-xs">沒有被拒的回報</div>
                  <ul class="q-my-none q-pl-lg">
                    <li>
                      認證碼不符的代理回報會在此依來源 IP
                      累計次數與最後嘗試時間。
                    </li>
                    <li>
                      後端未設定
                      <span class="mono-text">AGENT_AUTH_CODE</span>
                      時，入庫端點一律回 503（不列入此表）。
                    </li>
                  </ul>
                </div>
              </template>
            </q-table>
            <q-banner
              v-else-if="authFailuresError"
              dense
              rounded
              class="bg-negative text-white"
            >
              無法取得被拒回報：{{ authFailuresError }}
            </q-banner>
            <div v-else class="text-grey-7">載入中…</div>
          </q-card-section>
        </q-card>
      </div>
    </div>
  </q-page>
</template>

<script setup lang="ts">
import type { QTableProps } from "quasar";
import { computed, onMounted, ref } from "vue";

import {
  listAgentAuthFailures,
  listAgents,
  type Agent,
  type AgentAuthFailure
} from "@/api/agents";
import { fetchHealth, type HealthResponse } from "@/api/health";
import { getKeaStatus, type KeaStatus } from "@/api/kea";
import { relativeTime } from "@/utils/relativeTime";

const status = ref<KeaStatus | null>(null);
const health = ref<HealthResponse | null>(null);
const loading = ref(false);
const loadError = ref("");
const healthError = ref("");
const updatedAt = ref<Date | null>(null);

/** 觀測代理（null＝尚未成功載入；見票 01）。 */
const agents = ref<Agent[] | null>(null);
const agentsError = ref("");
/** 後端在線門檻秒數（`AGENT_STALE_SECS`）。 */
const staleSecs = ref(0);
/** 被拒回報（null＝尚未成功載入）。 */
const authFailures = ref<AgentAuthFailure[] | null>(null);
const authFailuresError = ref("");

const agentColumns: QTableProps["columns"] = [
  { name: "name", label: "名稱", field: "name", align: "left" },
  { name: "source_ip", label: "來源 IP", field: "source_ip", align: "left" },
  { name: "subnet", label: "涵蓋網段", field: "subnet_cidr", align: "left" },
  { name: "version", label: "版本", field: "version", align: "left" },
  {
    name: "last_report",
    label: "最後回報",
    field: "last_report_at",
    align: "left"
  },
  { name: "status", label: "狀態", field: "online", align: "left" }
];

const authFailureColumns: QTableProps["columns"] = [
  { name: "source_ip", label: "來源 IP", field: "source_ip", align: "left" },
  {
    name: "claimed",
    label: "自報名稱／版本",
    field: "claimed_name",
    align: "left"
  },
  {
    name: "attempt_count",
    label: "次數",
    field: "attempt_count",
    align: "left"
  },
  {
    name: "last_attempt",
    label: "最後嘗試",
    field: "last_attempt_at",
    align: "left"
  }
];

/** 代理狀態 chip：離線（不再回報）優先於未對應。 */
function agentStatus(agent: Agent): {
  label: string;
  color: string;
  textColor: string;
  hint?: string;
} {
  if (!agent.online) {
    return { label: "離線", color: "grey-7", textColor: "white" };
  }
  if (agent.subnet_id === null) {
    return {
      label: "未對應",
      color: "warning",
      textColor: "black",
      hint: "回報的涵蓋網段不在受管網段中，請建立對應網段或修正代理設定"
    };
  }
  return { label: "在線", color: "positive", textColor: "white" };
}

/** 版本顯示 `version ?? text`（真機 3.2.1 無 `arguments.version`，只回 text）。 */
const versionText = computed(() => {
  const version = status.value?.version;
  if (!version) {
    return null;
  }
  return version.version ?? version.text;
});

const uptimeText = computed(() => {
  const uptime = status.value?.runtime?.uptime ?? null;
  return uptime === null ? "—" : humanizeDuration(uptime);
});

const reloadText = computed(() => {
  const reload = status.value?.runtime?.reload ?? null;
  return reload === null ? "—" : humanizeAgo(reload);
});

/** socket 狀態（如 `ready`）；未提供為 null（不做綁定清單）。 */
const socketText = computed(
  () => status.value?.runtime?.sockets?.status ?? null
);

/** Kea 網段數與本地受管網段數不一致（診斷提示，不阻擋）。 */
const subnetCountMismatch = computed(() => {
  const dhcp4 = status.value?.dhcp4;
  return dhcp4 != null && dhcp4.subnet_count !== dhcp4.managed_subnet_count;
});

/** 相對秒數人化（如 183600 →「2 天 3 小時」）；不足 1 分鐘以秒計。 */
function humanizeDuration(totalSeconds: number): string {
  const days = Math.floor(totalSeconds / 86400);
  const hours = Math.floor((totalSeconds % 86400) / 3600);
  const minutes = Math.floor((totalSeconds % 3600) / 60);
  const seconds = totalSeconds % 60;
  const parts: string[] = [];
  if (days > 0) {
    parts.push(`${days} 天`);
  }
  if (hours > 0) {
    parts.push(`${hours} 小時`);
  }
  if (minutes > 0) {
    parts.push(`${minutes} 分鐘`);
  }
  if (parts.length === 0) {
    parts.push(`${seconds} 秒`);
  }
  return parts.join(" ");
}

/** 距上次設定重載的秒數 →「X 前」（0 為「剛剛」）。 */
function humanizeAgo(totalSeconds: number): string {
  return totalSeconds <= 0 ? "剛剛" : `${humanizeDuration(totalSeconds)}前`;
}

function formatTime(date: Date): string {
  return date.toLocaleTimeString("zh-TW", { hour12: false });
}

/** 精確時間：顯示瀏覽器本地時間；解析失敗原樣顯示（tooltip 用）。 */
function exactTime(value: string): string {
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? value : date.toLocaleString();
}

async function load() {
  loading.value = true;
  try {
    const [keaResult, healthResult, agentsResult, authFailuresResult] =
      await Promise.allSettled([
        getKeaStatus(),
        fetchHealth(),
        listAgents(),
        listAgentAuthFailures()
      ]);

    if (keaResult.status === "fulfilled") {
      status.value = keaResult.value;
      loadError.value = "";
    } else {
      status.value = null;
      loadError.value = errorMessage(keaResult.reason);
    }

    if (healthResult.status === "fulfilled") {
      health.value = healthResult.value;
      healthError.value = "";
    } else {
      health.value = null;
      healthError.value = errorMessage(healthResult.reason);
    }

    if (agentsResult.status === "fulfilled") {
      agents.value = agentsResult.value.items;
      staleSecs.value = agentsResult.value.stale_secs;
      agentsError.value = "";
    } else {
      agents.value = null;
      agentsError.value = errorMessage(agentsResult.reason);
    }

    if (authFailuresResult.status === "fulfilled") {
      authFailures.value = authFailuresResult.value.items;
      authFailuresError.value = "";
    } else {
      authFailures.value = null;
      authFailuresError.value = errorMessage(authFailuresResult.reason);
    }

    updatedAt.value = new Date();
  } finally {
    loading.value = false;
  }
}

function errorMessage(cause: unknown): string {
  return cause instanceof Error ? cause.message : String(cause);
}

onMounted(() => {
  void load();
});
</script>

<style scoped>
.mono-text {
  font-family: monospace;
}
</style>
