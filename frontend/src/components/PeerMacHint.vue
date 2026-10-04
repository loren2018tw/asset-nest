<template>
  <div class="text-caption" :class="mac === null ? 'text-grey-7' : ''">
    <template v-if="loading">讀取連線主機 MAC…</template>
    <template v-else-if="mac !== null">
      目前連線主機 MAC：<span class="text-mono">{{ mac }}</span
      >（可供手動填入）
      <q-btn dense flat size="sm" color="primary" label="填入" @click="fill" />
    </template>
    <template v-else> 無法取得連線主機 MAC（需與本系統同一層網路） </template>
  </div>
</template>

<script setup lang="ts">
import { onMounted, ref } from "vue";

import { fetchPeerMac } from "@/api/system";

const emit = defineEmits<{
  fill: [mac: string];
}>();

const loading = ref(true);
const mac = ref<string | null>(null);

onMounted(async () => {
  try {
    mac.value = (await fetchPeerMac()).mac;
  } catch {
    // 輔助資訊：查詢失敗視同查不到，不阻擋輸入
    mac.value = null;
  } finally {
    loading.value = false;
  }
});

function fill() {
  if (mac.value !== null) {
    emit("fill", mac.value);
  }
}
</script>
