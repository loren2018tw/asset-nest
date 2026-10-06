<template>
  <q-dialog v-model="open">
    <q-card
      style="width: 720px; max-width: 95vw; max-height: 90vh; overflow: auto"
    >
      <q-card-section class="row items-start q-pb-none">
        <div>
          <div class="text-h6">
            {{ isMacMode ? "MAC 觀測歷史" : "觀測歷史" }}
          </div>
          <div class="text-subtitle2 text-grey-7">
            {{ isMacMode ? currentMac : address }}
          </div>
        </div>
        <q-space />
        <q-btn
          v-if="!isMacMode"
          flat
          dense
          round
          icon="download"
          :loading="exporting"
          aria-label="匯出 CSV"
          @click="exportCsv"
        >
          <q-tooltip>匯出此 IP 的觀測歷史 CSV</q-tooltip>
        </q-btn>
      </q-card-section>

      <q-card-section class="q-pt-sm">
        <q-inner-loading :showing="loading">
          <q-spinner color="primary" size="32px" />
        </q-inner-loading>

        <!-- 以 IP 進入：現況、事件時間軸、用過的 MAC（見票 06） -->
        <template v-if="!isMacMode && history !== null">
          <q-banner
            v-if="!history.observed"
            dense
            rounded
            class="bg-grey-3 q-mb-md"
          >
            未觀測：此位址未被已啟用的觀測涵蓋（網段未開啟觀測、本機非同
            L2，或為 IPv6）。
          </q-banner>

          <template v-else>
            <div
              v-if="presence === null || presence.last_seen_at === null"
              class="q-mb-md"
            >
              <q-badge color="grey-7">從未上線</q-badge>
              <span class="text-grey-7 q-ml-sm">
                已開啟觀測並持續檢查；尚未看到此位址
                <template v-if="presence?.last_checked_at">
                  （最後檢查：{{ exactTime(presence.last_checked_at) }}）
                </template>
              </span>
            </div>
            <div v-else class="row q-col-gutter-sm q-mb-md">
              <div class="col-6 col-sm-3">
                <div class="text-caption text-grey-7">最後可見</div>
                <div>{{ relativeTime(presence.last_seen_at) }}</div>
                <div class="text-caption text-grey-7">
                  {{ exactTime(presence.last_seen_at) }}
                </div>
              </div>
              <div class="col-6 col-sm-3">
                <div class="text-caption text-grey-7">最後 MAC</div>
                <div class="mac-text">{{ presence.last_seen_mac ?? "—" }}</div>
              </div>
              <div class="col-6 col-sm-3">
                <div class="text-caption text-grey-7">來源</div>
                <div>{{ sourceLabel(presence.last_seen_source) }}</div>
              </div>
              <div class="col-6 col-sm-3">
                <div class="text-caption text-grey-7">最後檢查</div>
                <div>{{ exactTime(presence.last_checked_at) }}</div>
              </div>
            </div>
          </template>

          <div class="text-subtitle2 q-mb-xs">事件時間軸</div>
          <q-timeline v-if="history.events.length > 0" dense color="primary">
            <q-timeline-entry
              v-for="event in history.events"
              :key="event.id"
              :icon="event.kind === 'first_seen' ? 'fiber_new' : 'swap_horiz'"
              :title="kindLabel(event.kind)"
              :subtitle="`${sourceLabel(event.source)} ｜ ${relativeTime(event.observed_at)}`"
            >
              <div class="text-caption text-grey-7">
                {{ exactTime(event.observed_at) }}
                <template v-if="event.mac"> ｜ MAC {{ event.mac }}</template>
              </div>
            </q-timeline-entry>
          </q-timeline>
          <div v-else class="text-grey-7 q-mb-md">尚無變化事件。</div>

          <div class="text-subtitle2 q-mb-xs">用過的 MAC</div>
          <q-list
            v-if="history.macs.length > 0"
            dense
            bordered
            separator
            class="rounded-borders"
          >
            <q-item v-for="used in history.macs" :key="used.mac">
              <q-item-section>
                <q-item-label class="row items-center">
                  <span class="mac-text">{{ used.mac }}</span>
                  <q-badge
                    v-if="!used.known"
                    color="grey-7"
                    class="q-ml-sm"
                    label="未登錄"
                  />
                </q-item-label>
                <q-item-label v-if="used.asset" caption>
                  {{
                    assetLabel(used.asset.property_no, used.asset.description)
                  }}
                  ｜ {{ used.asset.location }}
                </q-item-label>
                <q-item-label caption>
                  首見 {{ exactTime(used.first_seen_at) }}（{{
                    relativeTime(used.first_seen_at)
                  }}）｜ 最後可見 {{ exactTime(used.last_seen_at) }}（{{
                    relativeTime(used.last_seen_at)
                  }}）｜ 來源 {{ sourceLabel(used.source) }}
                </q-item-label>
              </q-item-section>
            </q-item>
          </q-list>
          <div v-else class="text-grey-7">尚無用過的 MAC。</div>
        </template>

        <!-- 以 MAC 進入：已知資產與用過的位址（見票 06；資產端入口見票 08） -->
        <template v-else-if="isMacMode && macHistory !== null">
          <div class="row items-center q-mb-md">
            <template v-if="macHistory.known && macHistory.asset">
              <div>
                <div class="text-subtitle2">
                  {{
                    assetLabel(
                      macHistory.asset.property_no,
                      macHistory.asset.description
                    )
                  }}
                </div>
                <div class="text-caption text-grey-7">
                  {{ macHistory.asset.location }}
                </div>
              </div>
            </template>
            <q-badge v-else color="grey-7" label="未登錄" />
          </div>

          <div class="text-subtitle2 q-mb-xs">用過的位址</div>
          <q-list
            v-if="macHistory.sightings.length > 0"
            dense
            bordered
            separator
            class="rounded-borders"
          >
            <q-item
              v-for="sighting in macHistory.sightings"
              :key="sighting.address"
            >
              <q-item-section>
                <q-item-label class="mac-text">
                  {{ sighting.address }}
                </q-item-label>
                <q-item-label caption>
                  首見 {{ exactTime(sighting.first_seen_at) }}（{{
                    relativeTime(sighting.first_seen_at)
                  }}）｜ 最後可見 {{ exactTime(sighting.last_seen_at) }}（{{
                    relativeTime(sighting.last_seen_at)
                  }}）｜ 來源 {{ sourceLabel(sighting.source) }}
                </q-item-label>
              </q-item-section>
            </q-item>
          </q-list>
          <div v-else class="text-grey-7">此 MAC 尚無使用紀錄。</div>
        </template>
      </q-card-section>

      <q-card-actions align="right">
        <q-btn v-close-popup flat label="關閉" />
      </q-card-actions>
    </q-card>
  </q-dialog>
</template>

<script setup lang="ts">
import { useQuasar } from "quasar";
import { computed, ref, watch } from "vue";

import { saveBlob } from "@/api/client";
import type { IpSeenSource } from "@/api/ips";
import {
  downloadIpObservationsCsv,
  fetchIpObservations,
  fetchMacObservations,
  type IpObservationHistory,
  type MacObservationHistory,
  type ObservationEventKind
} from "@/api/observations";
import { assetLabel } from "@/utils/assetLabel";
import { relativeTime } from "@/utils/relativeTime";

/**
 * 觀測歷史對話框：支援「以 IP 進入」（`subnetId`＋`address`）與
 * 「以 MAC 進入」（`mac`；供資產介面點開，見票 08）。
 *
 * 以 IP 進入時顯示現況摘要、事件時間軸、用過的 MAC 與 CSV 匯出；
 * 以 MAC 進入時顯示已知資產與用過的位址。`mac` 有值時優先於 IP 模式。
 */
const props = defineProps<{
  modelValue: boolean;
  /** 以 IP 進入時提供（與 `mac` 二擇一）。 */
  subnetId?: number | undefined;
  address?: string | undefined;
  /** 以 MAC 進入時提供（資產介面的 MAC → 觀測歷史）。 */
  mac?: string | undefined;
}>();

const emit = defineEmits<{
  "update:modelValue": [value: boolean];
}>();

const $q = useQuasar();

const open = computed({
  get: () => props.modelValue,
  set: value => emit("update:modelValue", value)
});

const currentMac = computed(() => props.mac?.trim() ?? "");
const isMacMode = computed(() => currentMac.value !== "");
const address = computed(() => props.address ?? "");

const loading = ref(false);
const exporting = ref(false);
const history = ref<IpObservationHistory | null>(null);
const macHistory = ref<MacObservationHistory | null>(null);

const presence = computed(() => history.value?.presence ?? null);

watch(
  () => props.modelValue,
  value => {
    if (value) {
      void load();
    }
  }
);

/** 依模式載入歷史；失敗以 `$q.notify` 回報（見票 06）。 */
async function load() {
  history.value = null;
  macHistory.value = null;

  loading.value = true;
  try {
    if (isMacMode.value) {
      macHistory.value = await fetchMacObservations(currentMac.value);
    } else if (props.subnetId !== undefined && address.value !== "") {
      history.value = await fetchIpObservations(props.subnetId, address.value);
    }
  } catch (cause) {
    $q.notify({ type: "negative", message: messageOf(cause) });
  } finally {
    loading.value = false;
  }
}

/** 匯出目前 IP 的觀測歷史 CSV（UTF-8 BOM；僅 IP 模式）。 */
async function exportCsv() {
  if (props.subnetId === undefined || address.value === "") {
    return;
  }

  exporting.value = true;
  try {
    const file = await downloadIpObservationsCsv(props.subnetId, address.value);
    saveBlob(file.blob, file.filename ?? `觀測歷史_${address.value}.csv`);
  } catch (cause) {
    $q.notify({ type: "negative", message: messageOf(cause) });
  } finally {
    exporting.value = false;
  }
}

/** 觀測來源標籤（見票 02、ADR-0017）；未知或缺少顯示「—」。 */
function sourceLabel(source: IpSeenSource | null | undefined): string {
  switch (source) {
    case "arp":
      return "ARP";
    case "kea_lease":
      return "Kea 租約";
    case "arp_passive":
      return "ARP 被動";
    default:
      return "—";
  }
}

function kindLabel(kind: ObservationEventKind): string {
  return kind === "first_seen" ? "首見" : "MAC 變更";
}

/** 精確時間：顯示瀏覽器本地時間；缺值為「—」。 */
function exactTime(value: string | null | undefined): string {
  if (value === null || value === undefined) {
    return "—";
  }
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? value : date.toLocaleString();
}

function messageOf(cause: unknown): string {
  return cause instanceof Error ? cause.message : String(cause);
}
</script>

<style scoped>
.mac-text {
  font-family: monospace;
}
</style>
