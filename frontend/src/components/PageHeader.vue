<template>
  <header class="page-hero">
    <span
      class="page-hero__blob page-hero__blob--sage"
      aria-hidden="true"
    ></span>
    <span
      class="page-hero__blob page-hero__blob--clay"
      aria-hidden="true"
    ></span>
    <svg
      class="page-hero__art"
      viewBox="0 0 320 200"
      fill="none"
      aria-hidden="true"
    >
      <path
        d="M-20 210C60 200 140 170 200 120C250 78 290 30 320 -10"
        stroke="currentColor"
        stroke-width="1.5"
      />
      <path
        d="M-20 240C70 228 160 196 226 140C276 98 310 50 334 6"
        stroke="currentColor"
        stroke-width="1.5"
      />
      <path
        d="M-20 270C80 256 180 222 252 160C300 118 326 74 348 30"
        stroke="currentColor"
        stroke-width="1.5"
      />
    </svg>

    <div class="page-hero__main">
      <p class="page-hero__eyebrow">{{ eyebrow }}</p>
      <h1 class="page-hero__title">{{ title }}</h1>
      <div v-if="$slots.meta" class="page-hero__meta">
        <slot name="meta" />
      </div>
    </div>

    <div v-if="$slots.actions" class="page-hero__actions">
      <slot name="actions" />
    </div>
  </header>
</template>

<script setup lang="ts">
/** 全系統一致的頁首：眉標、標題、摘要資訊（meta）與右側操作（actions）。 */
defineProps<{
  eyebrow: string;
  title: string;
}>();
</script>

<style scoped>
.page-hero {
  position: relative;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: clamp(16px, 2.4vw, 32px);
  padding: clamp(20px, 2.2vw, 28px) clamp(22px, 2.6vw, 34px);
  overflow: hidden;
  border: 1px solid rgba(47, 93, 70, 0.09);
  border-radius: var(--an-radius-lg);
  background:
    radial-gradient(
      640px 260px at 103% -12%,
      rgba(126, 155, 130, 0.3),
      transparent 70%
    ),
    linear-gradient(138deg, #ffffff 8%, #f3f7ee 100%);
  box-shadow: var(--an-shadow);
}

.page-hero__blob {
  position: absolute;
  border-radius: 46% 54% 58% 42% / 52% 46% 54% 48%;
  pointer-events: none;
}

.page-hero__blob--sage {
  top: -96px;
  right: -70px;
  width: 240px;
  height: 240px;
  background: rgba(126, 155, 130, 0.22);
  filter: blur(2px);
}

.page-hero__blob--clay {
  bottom: -110px;
  left: 38%;
  width: 170px;
  height: 170px;
  background: rgba(176, 107, 67, 0.1);
  filter: blur(3px);
}

.page-hero__art {
  position: absolute;
  right: -30px;
  bottom: -40px;
  width: 340px;
  height: 220px;
  color: rgba(47, 93, 70, 0.16);
  pointer-events: none;
}

.page-hero__main {
  position: relative;
  z-index: 1;
  max-width: 640px;
  min-width: 0;
}

.page-hero__eyebrow {
  margin: 0;
  font-size: 11.5px;
  font-weight: 600;
  letter-spacing: 0.3em;
  text-transform: uppercase;
  color: var(--an-sage);
}

.page-hero__title {
  margin: 6px 0 0;
  font-size: clamp(24px, 2.4vw, 30px);
  font-weight: 700;
  line-height: 1.25;
  letter-spacing: 0.06em;
  color: var(--an-forest-deep);
}

.page-hero__meta {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px 12px;
  margin-top: 10px;
}

.page-hero__actions {
  position: relative;
  z-index: 1;
  display: flex;
  flex-wrap: wrap;
  gap: 10px;
}

.page-hero__actions :deep(.q-btn) {
  min-height: 42px;
  padding: 0 18px;
  font-weight: 500;
  letter-spacing: 0.04em;
  border-radius: 999px;
  transition:
    background-color 0.2s ease,
    border-color 0.2s ease,
    box-shadow 0.2s ease,
    color 0.2s ease;
}

.page-hero__actions :deep(.q-btn--flat),
.page-hero__actions :deep(.q-btn--outline) {
  color: var(--an-ink-soft);
  background: rgba(255, 255, 255, 0.72);
}

.page-hero__actions :deep(.q-btn--flat:hover),
.page-hero__actions :deep(.q-btn--outline:hover) {
  color: var(--an-forest);
  background: #ffffff;
}

.page-hero__actions :deep(.q-btn--unelevated) {
  box-shadow: 0 14px 28px -18px rgba(47, 93, 70, 0.85);
}

.page-hero__actions :deep(.q-btn:focus-visible) {
  outline: 2px solid var(--an-forest);
  outline-offset: 3px;
}

/* 行動版：操作按鈕佔滿整列，方便單手點擊 */
@media (max-width: 1023px) {
  .page-hero {
    align-items: flex-start;
    flex-direction: column;
  }

  .page-hero__actions {
    width: 100%;
  }

  .page-hero__actions :deep(.q-btn) {
    flex: 1 1 140px;
  }
}

@media (max-width: 599px) {
  .page-hero {
    padding: 20px;
    border-radius: 20px;
  }

  .page-hero__actions :deep(.q-btn) {
    flex: 1 1 100%;
  }
}

@media (prefers-reduced-motion: reduce) {
  .page-hero *,
  .page-hero *::before,
  .page-hero *::after {
    transition-duration: 0.01ms !important;
  }
}
</style>
