<template>
  <div class="login-page">
    <q-card flat class="login-card">
      <q-card-section class="login-card__head">
        <span class="login-card__mark">
          <img src="/plant.png" alt="" width="30" height="30" />
        </span>
        <div class="login-card__intro">
          <p class="login-card__eyebrow">Asset nest</p>
          <h1 class="login-card__title">IT 資產整合管理系統</h1>
        </div>
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
            class="login-card__submit full-width q-mt-lg"
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
.login-page {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 100%;
  min-height: 100vh;
  padding: 20px;
  background:
    radial-gradient(
      900px 420px at 104% -8%,
      rgba(126, 155, 130, 0.22),
      transparent 62%
    ),
    radial-gradient(
      720px 380px at -8% 6%,
      rgba(176, 107, 67, 0.08),
      transparent 58%
    ),
    var(--an-paper);
}

.login-card {
  width: 420px;
  max-width: calc(100vw - 32px);
  overflow: hidden;
  background: var(--an-surface);
  border: 1px solid var(--an-line-soft);
  border-radius: var(--an-radius-lg);
  box-shadow: var(--an-shadow);
}

.login-card__head {
  display: flex;
  gap: 14px;
  align-items: center;
  padding-bottom: 4px;
}

.login-card__mark {
  display: inline-flex;
  flex: none;
  align-items: center;
  justify-content: center;
  width: 46px;
  height: 46px;
  background: var(--an-sage-soft);
  border-radius: 46% 54% 58% 42% / 52% 46% 54% 48%;
}

.login-card__mark img {
  display: block;
  width: 30px;
  height: 30px;
}

.login-card__eyebrow {
  margin: 0;
  font-size: 11px;
  font-weight: 600;
  letter-spacing: 0.28em;
  text-transform: uppercase;
  color: var(--an-sage);
}

.login-card__title {
  margin: 4px 0 0;
  font-size: 18px;
  font-weight: 600;
  line-height: 1.4;
  letter-spacing: 0.04em;
  color: var(--an-forest-deep);
}

.login-card__submit {
  border-radius: 999px;
}
</style>
