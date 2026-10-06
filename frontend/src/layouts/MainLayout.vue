<template>
  <q-layout view="lHh Lpr lFf">
    <q-header elevated>
      <q-toolbar>
        <q-btn
          flat
          dense
          round
          icon="menu"
          aria-label="選單"
          @click="toggleLeftDrawer"
        />

        <q-toolbar-title> IT 資產整合管理系統 </q-toolbar-title>

        <div>Quasar v{{ $q.version }}</div>
      </q-toolbar>
    </q-header>

    <q-drawer v-model="leftDrawerOpen" show-if-above bordered>
      <q-list>
        <q-item clickable to="/assets" exact>
          <q-item-section avatar>
            <q-icon name="inventory_2" />
          </q-item-section>
          <q-item-section> 資產管理 </q-item-section>
        </q-item>
        <q-item clickable to="/ips" :active="ipSection">
          <q-item-section avatar>
            <q-icon name="lan" />
          </q-item-section>
          <q-item-section> IP 管理 </q-item-section>
        </q-item>
        <q-item
          clickable
          to="/observations/unmanaged"
          :active="unmanagedActive"
        >
          <q-item-section avatar>
            <q-icon name="travel_explore" />
          </q-item-section>
          <q-item-section> 網段外觀測 </q-item-section>
        </q-item>
        <q-item-label header> Kea </q-item-label>
        <q-item clickable to="/kea/leases" :active="keaLeasesActive">
          <q-item-section avatar>
            <q-icon name="receipt_long" />
          </q-item-section>
          <q-item-section> 租約清單 </q-item-section>
        </q-item>
        <q-item clickable to="/kea/status" :active="keaStatusActive">
          <q-item-section avatar>
            <q-icon name="monitor_heart" />
          </q-item-section>
          <q-item-section> 系統狀態 </q-item-section>
        </q-item>
      </q-list>
    </q-drawer>

    <q-page-container>
      <router-view />
    </q-page-container>
  </q-layout>
</template>

<script setup lang="ts">
import { computed, ref } from "vue";
import { useRoute } from "vue-router";

const route = useRoute();

const leftDrawerOpen = ref(false);

/** IP 管理區段：網段列表（/ips）與各網段的 IP 頁（/subnets/:id/ips）。 */
const ipSection = computed(
  () => route.path.startsWith("/ips") || route.path.startsWith("/subnets/")
);

/** 網段外觀測（/observations/unmanaged）：IP 管理區段新增項目，獨立高亮。 */
const unmanagedActive = computed(() =>
  route.path.startsWith("/observations/unmanaged")
);

/** Kea 區段（/kea 前綴）：租約清單與系統狀態兩頁各自保持高亮。 */
const keaLeasesActive = computed(() => route.path.startsWith("/kea/leases"));
const keaStatusActive = computed(() => route.path.startsWith("/kea/status"));

function toggleLeftDrawer() {
  leftDrawerOpen.value = !leftDrawerOpen.value;
}
</script>
