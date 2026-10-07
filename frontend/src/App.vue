<template>
  <router-view />
</template>

<script setup lang="ts">
import { useQuasar } from "quasar";
import { useRouter } from "vue-router";

import { setUnauthorizedHandler } from "@/api/client";
import { useAuthStore } from "@/stores/auth";

const $q = useQuasar();
const router = useRouter();
const auth = useAuthStore();

/**
 * 全域 401 攔截（見 spec §5.1）：已登入者收到 401＝工作階段失效，
 * 清除狀態、提示並帶 `redirect` 導向登入頁；未登入者（守衛初始化查詢）
 * 不提示、不導向，交由路由守衛處理。
 */
setUnauthorizedHandler(() => {
  if (auth.username === null) {
    return;
  }

  auth.expire();
  $q.notify({ type: "warning", message: "登入已過期，請重新登入" });

  void router.push({
    path: "/login",
    query: { redirect: router.currentRoute.value.fullPath }
  });
});
</script>
