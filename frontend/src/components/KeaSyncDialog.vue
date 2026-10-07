<template>
  <q-dialog v-model="open" persistent>
    <q-card style="min-width: 640px; max-width: 90vw">
      <q-card-section class="row items-center q-pb-none">
        <div class="text-h6">Kea 完整同步</div>
        <q-space />
        <q-btn
          flat
          dense
          round
          icon="close"
          aria-label="關閉"
          :disable="applying"
          @click="open = false"
        />
      </q-card-section>

      <q-card-section v-if="loading" class="row items-center">
        <q-spinner color="primary" />
        <span class="q-ml-sm">讀取同步計畫…</span>
      </q-card-section>

      <q-card-section v-else-if="loadError">
        <q-banner class="bg-negative text-white" rounded>
          {{ loadError }}
        </q-banner>
      </q-card-section>

      <q-card-section v-else-if="report" class="q-gutter-y-md">
        <q-banner class="bg-grey-2 text-grey-8" rounded>
          完成：新增 {{ reportTotals.added }}／更新
          {{ reportTotals.updated }}／刪除 {{ reportTotals.deleted }}；位址池
          新增 {{ reportTotals.poolAdded }}／刪除
          {{ reportTotals.poolDeleted }}、gateway 更新
          {{ reportTotals.gateways }} 個網段、已建立網段
          {{ reportTotals.subnetAdded }}；Kea 設定檔寫入：{{
            configWriteLabel
          }}。
        </q-banner>
        <div v-for="subnet in report.subnets" :key="subnet.subnet_id">
          <div class="text-subtitle2">{{ label(subnet) }}</div>
          <div v-if="subnet.error" class="text-negative">
            {{ subnet.error }}
          </div>
          <template v-else>
            <div class="text-body2">
              新增 {{ subnet.added }}／更新 {{ subnet.updated }}／刪除
              {{ subnet.deleted }}（跳過 {{ subnet.skipped }}）；位址池 新增
              {{ subnet.pool_added }}／刪除 {{ subnet.pool_deleted
              }}<span v-if="subnet.gateway_updated">、gateway 已更新</span>
              <span v-if="subnet.subnet_added">、已建立 Kea 網段</span>
            </div>
            <div
              v-if="subnet.subnet_add_error"
              class="text-negative text-body2"
            >
              {{ subnet.subnet_add_error }}
            </div>
            <div v-if="subnet.settings_error" class="text-negative text-body2">
              網段層設定同步失敗：{{ subnet.settings_error }}
            </div>
            <q-list
              v-if="subnet.failures.length > 0"
              dense
              class="text-negative text-body2"
            >
              <q-item
                v-for="(failure, index) in subnet.failures"
                :key="index"
                dense
              >
                <q-item-section>
                  {{ failureAction(failure.action) }}
                  {{ failure.ip_address }}：{{ failure.message }}
                </q-item-section>
              </q-item>
            </q-list>
          </template>
        </div>
      </q-card-section>

      <q-card-section v-else-if="plan" class="q-gutter-y-md">
        <q-banner
          v-if="plan.subnets.length === 0"
          class="bg-grey-2 text-grey-8"
          rounded
        >
          沒有受管網段（未設定 Kea subnet-id），無需同步。
        </q-banner>
        <template v-else>
          <q-banner class="bg-grey-2 text-grey-8" rounded>
            將以 asset-nest 為準對齊 Kea：新增 {{ plan.totals.add }}／更新
            {{ plan.totals.update }}／刪除 {{ plan.totals.delete }}／跳過
            {{ plan.totals.skipped }}；位址池 新增
            {{ plan.totals.pool_add }}／刪除
            {{ plan.totals.pool_delete }}、gateway 變更
            {{ plan.totals.gateway }}、新增網段
            {{ plan.totals.subnet_add }}。刪除只限受管網段中多餘的保留與位址池。
          </q-banner>
          <div v-for="subnet in plan.subnets" :key="subnet.subnet_id">
            <div class="text-subtitle2">{{ label(subnet) }}</div>
            <div v-if="subnet.error" class="text-negative">
              {{ subnet.error }}
            </div>
            <template v-else>
              <div class="text-body2">
                新增 {{ subnet.add.length }}／更新
                {{ subnet.update.length }}／刪除
                {{ subnet.delete.length }}／跳過
                {{ subnet.skipped.length }}；位址池 新增
                {{ subnet.pool_add.length }}／刪除 {{ subnet.pool_delete.length
                }}<span v-if="subnet.gateway">、gateway 變更</span>
                <span v-if="subnet.subnet_add">、將建立 Kea 網段</span>
              </div>
              <q-expansion-item
                v-if="hasDetail(subnet)"
                dense
                icon="list"
                label="查看明細"
              >
                <q-list dense class="text-body2">
                  <q-item v-if="subnet.subnet_add" dense>
                    <q-item-section>
                      <div>
                        新增網段（Kea id
                        <span class="text-mono">{{ subnet.kea_subnet_id }}</span
                        >）
                      </div>
                      <div
                        v-for="range in subnet.subnet_add.pools"
                        :key="`subnet-add-${range}`"
                        class="q-pl-md"
                      >
                        位址池 <span class="text-mono">{{ range }}</span>
                      </div>
                      <div
                        v-if="subnet.subnet_add.pools.length === 0"
                        class="q-pl-md"
                      >
                        無位址池
                      </div>
                      <div class="q-pl-md">
                        gateway
                        <span class="text-mono">{{
                          subnet.subnet_add.gateway ?? "（未設）"
                        }}</span>
                      </div>
                    </q-item-section>
                  </q-item>
                  <q-item
                    v-for="item in subnet.add"
                    :key="`add-${item.ip_address}`"
                    dense
                  >
                    <q-item-section>
                      新增
                      <span class="text-mono">{{ item.ip_address }}</span> （{{
                        item.desired?.hw_address
                      }}）
                    </q-item-section>
                  </q-item>
                  <q-item
                    v-for="item in subnet.update"
                    :key="`update-${item.ip_address}`"
                    dense
                  >
                    <q-item-section>
                      更新
                      <span class="text-mono">{{ item.ip_address }}</span> ：{{
                        item.current?.hw_address
                      }}
                      → {{ item.desired?.hw_address }}／hostname
                      {{ item.current?.hostname ?? "—" }} →
                      {{ item.desired?.hostname ?? "—" }}
                    </q-item-section>
                  </q-item>
                  <q-item
                    v-for="item in subnet.delete"
                    :key="`delete-${item.ip_address}`"
                    dense
                  >
                    <q-item-section>
                      刪除
                      <span class="text-mono">{{ item.ip_address }}</span> （{{
                        item.current?.hw_address
                      }}）
                    </q-item-section>
                  </q-item>
                  <q-item
                    v-for="range in subnet.pool_add"
                    :key="`pool-add-${range}`"
                    dense
                  >
                    <q-item-section>
                      新增位址池 <span class="text-mono">{{ range }}</span>
                    </q-item-section>
                  </q-item>
                  <q-item
                    v-for="range in subnet.pool_delete"
                    :key="`pool-delete-${range}`"
                    dense
                  >
                    <q-item-section>
                      刪除位址池 <span class="text-mono">{{ range }}</span>
                    </q-item-section>
                  </q-item>
                  <q-item v-if="subnet.gateway" dense>
                    <q-item-section>
                      gateway
                      <span class="text-mono">{{
                        gatewayText(subnet.gateway)
                      }}</span>
                    </q-item-section>
                  </q-item>
                  <q-item
                    v-for="(skip, index) in subnet.skipped"
                    :key="`skip-${index}`"
                    dense
                  >
                    <q-item-section class="text-warning">
                      跳過 {{ skip.ip_address ?? "（無位址）" }}：{{
                        skip.reason
                      }}
                    </q-item-section>
                  </q-item>
                </q-list>
              </q-expansion-item>
            </template>
          </div>
        </template>
      </q-card-section>

      <q-separator />
      <q-card-actions align="right">
        <q-btn flat label="關閉" :disable="applying" @click="open = false" />
        <q-btn
          v-if="report === null"
          color="primary"
          label="套用同步"
          :loading="applying"
          :disable="!canApply"
          @click="apply"
        />
      </q-card-actions>
    </q-card>
  </q-dialog>
</template>

<script setup lang="ts">
import { useQuasar } from "quasar";
import { computed, ref, watch } from "vue";

import {
  applySync,
  getSyncPlan,
  type KeaApplyReport,
  type KeaGatewayPlan,
  type KeaPlanSubnet,
  type KeaSyncPlan
} from "@/api/kea";

const props = defineProps<{ modelValue: boolean }>();
const emit = defineEmits<{ "update:modelValue": [value: boolean] }>();

const $q = useQuasar();

const open = computed({
  get: () => props.modelValue,
  set: (value: boolean) => emit("update:modelValue", value)
});

const loading = ref(false);
const applying = ref(false);
const loadError = ref("");
const plan = ref<KeaSyncPlan | null>(null);
const report = ref<KeaApplyReport | null>(null);

const canApply = computed(() => {
  const totals = plan.value?.totals;
  if (totals === undefined) {
    return false;
  }
  return (
    totals.add +
      totals.update +
      totals.delete +
      totals.pool_add +
      totals.pool_delete +
      totals.gateway +
      totals.subnet_add >
    0
  );
});

const reportTotals = computed(() => {
  const totals = {
    added: 0,
    updated: 0,
    deleted: 0,
    poolAdded: 0,
    poolDeleted: 0,
    gateways: 0,
    subnetAdded: 0
  };
  for (const subnet of report.value?.subnets ?? []) {
    totals.added += subnet.added;
    totals.updated += subnet.updated;
    totals.deleted += subnet.deleted;
    totals.poolAdded += subnet.pool_added;
    totals.poolDeleted += subnet.pool_deleted;
    if (subnet.gateway_updated) {
      totals.gateways += 1;
    }
    if (subnet.subnet_added) {
      totals.subnetAdded += 1;
    }
  }
  return totals;
});

const configWriteLabel = computed(() => {
  const value = report.value?.config_write;
  if (value === "ok") {
    return "成功";
  }
  if (value === "failed") {
    return `失敗（${report.value?.config_write_message ?? "未知錯誤"}）`;
  }
  return "未變更，略過";
});

watch(
  () => props.modelValue,
  value => {
    if (value) {
      void loadPlan();
    }
  }
);

async function loadPlan() {
  loading.value = true;
  loadError.value = "";
  plan.value = null;
  report.value = null;
  try {
    plan.value = await getSyncPlan();
  } catch (cause) {
    loadError.value = messageOf(cause);
  } finally {
    loading.value = false;
  }
}

async function apply() {
  applying.value = true;
  try {
    report.value = await applySync();
    const failed =
      report.value.config_write === "failed" ||
      report.value.subnets.some(
        subnet =>
          subnet.failures.length > 0 ||
          Boolean(subnet.settings_error) ||
          Boolean(subnet.subnet_add_error)
      );
    $q.notify({
      type: failed ? "warning" : "positive",
      message: failed ? "完整同步完成，但有部分失敗" : "完整同步完成"
    });
  } catch (cause) {
    $q.notify({ type: "negative", message: messageOf(cause) });
  } finally {
    applying.value = false;
  }
}

function messageOf(cause: unknown): string {
  return cause instanceof Error ? cause.message : String(cause);
}

function label(subnet: { name: string | null; cidr: string }): string {
  return subnet.name ? `${subnet.name}（${subnet.cidr}）` : subnet.cidr;
}

function failureAction(action: string): string {
  switch (action) {
    case "add":
      return "新增";
    case "update":
      return "更新";
    case "delete":
      return "刪除";
    default:
      return action;
  }
}

/** 明細是否有內容可展開（保留或網段層差異）。 */
function hasDetail(subnet: KeaPlanSubnet): boolean {
  const items =
    subnet.add.length +
    subnet.update.length +
    subnet.delete.length +
    subnet.skipped.length +
    subnet.pool_add.length +
    subnet.pool_delete.length;
  return items > 0 || Boolean(subnet.gateway) || Boolean(subnet.subnet_add);
}

/** gateway 變更顯示：null 以「未設」／「移除」呈現。 */
function gatewayText(change: KeaGatewayPlan): string {
  return `${change.current ?? "（未設）"} → ${change.desired ?? "（移除）"}`;
}
</script>
