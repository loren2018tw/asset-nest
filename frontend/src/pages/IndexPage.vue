<template>
  <q-page class="flex flex-center">
    <q-card class="q-pa-lg" style="width: 420px; max-width: 90vw">
      <div class="text-h6">系統骨架狀態</div>
      <div class="text-caption text-grey-7 q-mb-md">
        前端（Quasar）與後端（Rust）連線檢查
      </div>

      <q-banner v-if="error" class="bg-negative text-white" dense rounded>
        無法連線後端：{{ error }}
      </q-banner>

      <q-list v-else-if="health" separator bordered rounded>
        <q-item>
          <q-item-section>
            <q-item-label caption>狀態</q-item-label>
            <q-item-label>{{ health.status }}</q-item-label>
          </q-item-section>
        </q-item>
        <q-item>
          <q-item-section>
            <q-item-label caption>服務</q-item-label>
            <q-item-label>{{ health.service }}</q-item-label>
          </q-item-section>
        </q-item>
        <q-item>
          <q-item-section>
            <q-item-label caption>版本</q-item-label>
            <q-item-label>{{ health.version }}</q-item-label>
          </q-item-section>
        </q-item>
        <q-item>
          <q-item-section>
            <q-item-label caption>資料庫</q-item-label>
            <q-item-label>{{ health.database }}</q-item-label>
          </q-item-section>
        </q-item>
      </q-list>

      <div v-else class="flex flex-center q-pa-md">
        <q-spinner color="primary" size="32px" />
      </div>

      <div class="row justify-end q-mt-md">
        <q-btn
          color="primary"
          label="重新檢查"
          :loading="loading"
          @click="load"
        />
      </div>
    </q-card>
  </q-page>
</template>

<script setup lang="ts">
import { onMounted, ref } from "vue";

import { fetchHealth, type HealthResponse } from "@/api/health";

const health = ref<HealthResponse | null>(null);
const error = ref("");
const loading = ref(false);

async function load() {
  loading.value = true;
  error.value = "";

  try {
    health.value = await fetchHealth();
  } catch (cause) {
    health.value = null;
    error.value = cause instanceof Error ? cause.message : String(cause);
  } finally {
    loading.value = false;
  }
}

onMounted(load);
</script>
