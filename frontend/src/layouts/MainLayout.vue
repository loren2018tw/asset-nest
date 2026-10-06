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
        <q-item clickable to="/ips" exact :active="ipSection">
          <q-item-section avatar>
            <q-icon name="lan" />
          </q-item-section>
          <q-item-section> IP 管理 </q-item-section>
        </q-item>
        <!-- 已設定網段的快速入口（見票 23）：每網段一項，連往該網段 IP 清單 -->
        <q-item
          v-for="subnet in subnets"
          :key="subnet.id"
          clickable
          dense
          :to="`/subnets/${subnet.id}/ips`"
          :active="activeSubnetId === subnet.id"
        >
          <q-item-section avatar>
            <q-icon name="format_list_numbered" />
          </q-item-section>
          <q-item-section>
            <q-item-label class="ellipsis">{{
              subnet.name || subnet.cidr
            }}</q-item-label>
            <q-item-label v-if="subnet.name" caption class="ellipsis">
              {{ subnet.cidr }}
            </q-item-label>
          </q-item-section>
        </q-item>
        <q-item
          clickable
          to="/observations/out-of-subnet"
          :active="outOfSubnetActive"
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
import { computed, onMounted, ref, watch } from "vue";
import { useRoute } from "vue-router";

import { listSubnets, type SubnetSummary } from "@/api/subnets";

const route = useRoute();

const leftDrawerOpen = ref(false);

/** 已設定網段（側欄快速入口；見票 23）。 */
const subnets = ref<SubnetSummary[]>([]);

/** IP 管理（網段列表 /ips）；各網段 IP 頁由下方的網段項目個別高亮。 */
const ipSection = computed(() => route.path.startsWith("/ips"));

/** 目前所在網段 id（/subnets/:id/ips）；其他頁面為 null。 */
const activeSubnetId = computed<number | null>(() => {
  if (!route.path.startsWith("/subnets/")) {
    return null;
  }
  const id = Number(route.params.id);
  return Number.isInteger(id) && id > 0 ? id : null;
});

/** 側欄網段清單：載入失敗僅略過（輔助入口，不干擾目前頁面）。 */
async function loadSubnets() {
  try {
    subnets.value = await listSubnets();
  } catch {
    // 側欄為快速入口；失敗時維持現有清單
  }
}

// 每次換頁重新載入，讓新增／刪除／改名後的網段在側欄保持最新
watch(
  () => route.path,
  () => {
    void loadSubnets();
  }
);

onMounted(() => {
  void loadSubnets();
});

/** 網段外觀測（/observations/out-of-subnet）：IP 管理區段新增項目，獨立高亮。 */
const outOfSubnetActive = computed(() =>
  route.path.startsWith("/observations/out-of-subnet")
);

/** Kea 區段（/kea 前綴）：租約清單與系統狀態兩頁各自保持高亮。 */
const keaLeasesActive = computed(() => route.path.startsWith("/kea/leases"));
const keaStatusActive = computed(() => route.path.startsWith("/kea/status"));

function toggleLeftDrawer() {
  leftDrawerOpen.value = !leftDrawerOpen.value;
}
</script>
