<template>
  <div
    class="window-height window-width row justify-center items-center bg-grey-2"
  >
    <q-card flat bordered class="login-card">
      <q-card-section class="text-center q-pb-none">
        <div class="text-h5 text-weight-medium">IT 資產整合管理系統</div>
        <div class="text-subtitle1 text-grey-7 q-mt-xs">登入</div>
      </q-card-section>

      <q-card-section>
        <q-form @submit="onSubmit">
          <q-input
            v-model="username"
            label="帳號"
            autocomplete="username"
            outlined
            dense
            autofocus
            :disable="submitting"
          />
          <q-input
            v-model="password"
            label="密碼"
            type="password"
            autocomplete="current-password"
            outlined
            dense
            class="q-mt-md"
            :disable="submitting"
          />

          <q-banner
            v-if="errorMessage"
            dense
            rounded
            class="bg-negative text-white q-mt-md"
          >
            {{ errorMessage }}
          </q-banner>

          <q-btn
            type="submit"
            color="primary"
            label="登入"
            class="full-width q-mt-lg"
            :loading="submitting"
            :disable="submitting"
          />
        </q-form>
      </q-card-section>
    </q-card>
  </div>
</template>

<script setup lang="ts">
import { ref } from "vue";
import { useRoute, useRouter } from "vue-router";

import { ApiError } from "@/api/client";
import { useAuthStore } from "@/stores/auth";

const route = useRoute();
const router = useRouter();
const auth = useAuthStore();

const username = ref("");
const password = ref("");
const submitting = ref(false);
const errorMessage = ref<string | null>(null);

/**
 * 只接受站內路徑（防開放轉址）：以 `/` 起頭、非 `//`、不含 `\`；
 * 其餘（外部網址、陣列等）一律視為無效。
 */
function safeRedirect(value: unknown): string | null {
  const target = Array.isArray(value) ? value[0] : value;
  if (typeof target !== "string") {
    return null;
  }

  if (
    !target.startsWith("/") ||
    target.startsWith("//") ||
    target.includes("\\")
  ) {
    return null;
  }

  return target;
}

/** 送出登入；Enter 由 `q-form` 的 submit 觸發（見 spec §5.2）。 */
async function onSubmit(): Promise<void> {
  if (submitting.value) {
    return;
  }

  submitting.value = true;
  errorMessage.value = null;

  try {
    await auth.login(username.value, password.value);
    await router.replace(safeRedirect(route.query.redirect) ?? "/kea/status");
  } catch (error) {
    // 後端登入失敗固定回「帳號或密碼錯誤」；非 API 錯誤（如連線失敗）給通用訊息
    errorMessage.value =
      error instanceof ApiError ? error.message : "無法連線，請稍後再試";
  } finally {
    submitting.value = false;
  }
}
</script>

<style scoped>
.login-card {
  width: 400px;
  max-width: calc(100vw - 32px);
}
</style>
