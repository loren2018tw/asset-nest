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

        <!-- 右上角資訊叢集（見 spec §5.3）：Loren → GitHub → 版本 → 登出 -->
        <div class="row items-center">
          <div class="text-caption">Loren</div>
          <q-btn
            flat
            dense
            round
            type="a"
            :href="'https://github.com/loren2018tw/asset-nest'"
            target="_blank"
            rel="noopener"
            aria-label="GitHub 專案首頁"
          >
            <svg
              aria-hidden="true"
              viewBox="0 0 16 16"
              width="20"
              height="20"
              fill="currentColor"
            >
              <path
                d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27s1.36.09 2 .27c1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.01 8.01 0 0 0 16 8c0-4.42-3.58-8-8-8z"
              />
            </svg>
          </q-btn>
          <div v-if="version" class="text-caption">V{{ version }}</div>
          <q-separator vertical class="q-mx-sm" />
          <q-btn
            flat
            dense
            round
            icon="logout"
            aria-label="登出"
            @click="onLogout"
          >
            <q-tooltip>登出</q-tooltip>
          </q-btn>
        </div>
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
        <q-item clickable to="/lendings" :active="lendingsActive">
          <q-item-section avatar>
            <q-icon name="swap_horiz" />
          </q-item-section>
          <q-item-section> 資產借還 </q-item-section>
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
import { useQuasar } from "quasar";
import { useRoute, useRouter } from "vue-router";

import { fetchHealth } from "@/api/health";
import { listSubnets, type SubnetSummary } from "@/api/subnets";
import { useAuthStore } from "@/stores/auth";

const $q = useQuasar();
const route = useRoute();
const router = useRouter();
const auth = useAuthStore();

const leftDrawerOpen = ref(false);

/** 後端版本號（顯示為 `V{version}`；取得失敗時為 null，不顯示）。 */
const version = ref<string | null>(null);

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

/** 右上角版本號：以 `GET /api/health` 為真實來源（見 spec §5.3）；失敗不顯示。 */
async function loadVersion() {
  try {
    const health = await fetchHealth();
    version.value = health.version;
  } catch {
    // 版本僅為顯示資訊；取得失敗時不顯示
  }
}

/** 登出（見 spec §5.3）：成功通知並轉登入頁；失敗維持登入狀態。 */
async function onLogout(): Promise<void> {
  try {
    await auth.logout();
    $q.notify({ type: "positive", message: "已登出" });
    await router.replace("/login");
  } catch {
    $q.notify({ type: "negative", message: "登出失敗，請重試" });
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
  void loadVersion();
});

/** 網段外觀測（/observations/out-of-subnet）：IP 管理區段新增項目，獨立高亮。 */
const outOfSubnetActive = computed(() =>
  route.path.startsWith("/observations/out-of-subnet")
);

/** 資產借還（/lendings）：資產管理區段、資產清單之下（見 asset-lending 票 04）。 */
const lendingsActive = computed(() => route.path.startsWith("/lendings"));

/** Kea 區段（/kea 前綴）：租約清單與系統狀態兩頁各自保持高亮。 */
const keaLeasesActive = computed(() => route.path.startsWith("/kea/leases"));
const keaStatusActive = computed(() => route.path.startsWith("/kea/status"));

function toggleLeftDrawer() {
  leftDrawerOpen.value = !leftDrawerOpen.value;
}
</script>
