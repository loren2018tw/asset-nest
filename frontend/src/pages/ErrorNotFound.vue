<template>
  <div class="not-found">
    <span class="not-found__mark">
      <img src="/plant.png" alt="" width="34" height="34" />
    </span>
    <p class="not-found__code">404</p>
    <h1 class="not-found__title">找不到這個頁面</h1>
    <p class="not-found__text">連結可能已失效，或頁面已被移動。</p>
    <q-btn
      class="not-found__btn"
      color="primary"
      unelevated
      no-caps
      icon="home"
      to="/"
      label="回到系統狀態"
    />
  </div>
</template>

<style scoped>
.not-found {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 6px;
  min-height: 100vh;
  padding: 24px;
  text-align: center;
  background:
    radial-gradient(
      900px 420px at 104% -8%,
      rgba(126, 155, 130, 0.2),
      transparent 62%
    ),
    var(--an-paper);
}

.not-found__mark {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 64px;
  height: 64px;
  margin-bottom: 8px;
  background: var(--an-sage-soft);
  border-radius: 46% 54% 58% 42% / 52% 46% 54% 48%;
}

.not-found__mark img {
  display: block;
  width: 34px;
  height: 34px;
}

.not-found__code {
  margin: 0;
  font-size: clamp(48px, 10vw, 96px);
  font-weight: 700;
  line-height: 1;
  letter-spacing: 0.08em;
  color: var(--an-sage);
}

.not-found__title {
  margin: 10px 0 0;
  font-size: 20px;
  font-weight: 600;
  line-height: 1.45;
  color: var(--an-forest-deep);
}

.not-found__text {
  margin: 6px 0 0;
  font-size: 13.5px;
  color: var(--an-muted);
}

.not-found__btn {
  margin-top: 18px;
  border-radius: 999px;
}
</style>
