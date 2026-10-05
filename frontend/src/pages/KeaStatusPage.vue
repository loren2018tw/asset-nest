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
    </div>
  </q-page>
</template>

<script setup lang="ts">
import { computed, onMounted, ref } from "vue";

import { fetchHealth, type HealthResponse } from "@/api/health";
import { getKeaStatus, type KeaStatus } from "@/api/kea";

const status = ref<KeaStatus | null>(null);
const health = ref<HealthResponse | null>(null);
const loading = ref(false);
const loadError = ref("");
const healthError = ref("");
const updatedAt = ref<Date | null>(null);

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

async function load() {
  loading.value = true;
  try {
    const [keaResult, healthResult] = await Promise.allSettled([
      getKeaStatus(),
      fetchHealth()
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
