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
          {{ reportTotals.updated }}／刪除 {{ reportTotals.deleted }}；Kea
          設定檔寫入：{{ configWriteLabel }}。
        </q-banner>
        <div v-for="subnet in report.subnets" :key="subnet.subnet_id">
          <div class="text-subtitle2">{{ label(subnet) }}</div>
          <div v-if="subnet.error" class="text-negative">
            {{ subnet.error }}
          </div>
          <template v-else>
            <div class="text-body2">
              新增 {{ subnet.added }}／更新 {{ subnet.updated }}／刪除
              {{ subnet.deleted }}（跳過 {{ subnet.skipped }}）
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
            {{ plan.totals.skipped }}。刪除只限受管網段中多餘的保留。
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
                {{ subnet.delete.length }}／跳過 {{ subnet.skipped.length }}
              </div>
              <q-expansion-item dense icon="list" label="查看明細">
                <q-list dense class="text-body2">
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
  return totals !== undefined && totals.add + totals.update + totals.delete > 0;
});

const reportTotals = computed(() => {
  const totals = { added: 0, updated: 0, deleted: 0 };
  for (const subnet of report.value?.subnets ?? []) {
    totals.added += subnet.added;
    totals.updated += subnet.updated;
    totals.deleted += subnet.deleted;
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
      report.value.subnets.some(subnet => subnet.failures.length > 0);
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
</script>
