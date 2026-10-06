<template>
  <q-dialog v-model="open" persistent>
    <q-card
      style="width: 560px; max-width: 95vw; max-height: 90vh; overflow: auto"
    >
      <q-form @submit="submit">
        <q-card-section>
          <div class="text-h6">借出資產</div>
          <div class="text-subtitle2 text-grey-7">
            {{ assetLabel(asset.property_no, asset.description) }}（{{
              asset.location
            }}）
          </div>
        </q-card-section>

        <q-card-section class="q-gutter-y-sm">
          <!-- 借用人：既有借用人下拉建議，仍可自由輸入新名 -->
          <q-select
            v-model="borrower"
            :options="borrowerOptions"
            use-input
            hide-selected
            fill-input
            input-debounce="300"
            outlined
            dense
            label="借用人 *"
            hint="可選既有借用人或輸入新名字"
            :rules="[borrowerRule]"
            @filter="filterBorrowers"
            @input="onBorrowerInput"
          >
            <template #no-option>
              <q-item>
                <q-item-section class="text-grey-6">
                  無相符借用人
                </q-item-section>
              </q-item>
            </template>
          </q-select>

          <q-input
            v-model="dueAt"
            type="date"
            outlined
            dense
            label="預計歸還日"
            hint="選填"
          />

          <q-input
            v-model="note"
            type="textarea"
            outlined
            dense
            autogrow
            label="備註"
            hint="選填"
          />
        </q-card-section>

        <q-card-actions align="right">
          <q-btn v-close-popup flat label="取消" />
          <q-btn color="primary" type="submit" label="借出" :loading="saving" />
        </q-card-actions>
      </q-form>
    </q-card>
  </q-dialog>
</template>

<script setup lang="ts">
import { useQuasar } from "quasar";
import { computed, ref, watch } from "vue";

import type { Asset } from "@/api/assets";
import { createLending, listBorrowers } from "@/api/lendings";
import { assetLabel } from "@/utils/assetLabel";

const props = defineProps<{
  modelValue: boolean;
  asset: Asset;
}>();

const emit = defineEmits<{
  "update:modelValue": [value: boolean];
  saved: [];
}>();

const $q = useQuasar();

const open = computed({
  get: () => props.modelValue,
  set: value => emit("update:modelValue", value)
});

const borrower = ref("");
/** 借用人建議（後端去重、最近使用者在前）；篩選後供 q-select 顯示。 */
const allBorrowers = ref<string[]>([]);
const borrowerOptions = ref<string[]>([]);
const dueAt = ref("");
const note = ref("");

const saving = ref(false);

/** 供借用人建議載入丟棄過期回應。 */
let loadToken = 0;

// immediate：由父層以 v-if 掛載並直接開啟時（modelValue 初始為 true）也要載入
watch(
  () => props.modelValue,
  value => {
    if (value) {
      void prepare();
    }
  },
  { immediate: true }
);

/** 對話框開啟時重設欄位並載入借用人建議。 */
async function prepare() {
  const token = ++loadToken;
  borrower.value = "";
  dueAt.value = "";
  note.value = "";
  allBorrowers.value = [];
  borrowerOptions.value = [];

  try {
    const borrowers = await listBorrowers();
    if (token !== loadToken) {
      return;
    }
    allBorrowers.value = borrowers;
    borrowerOptions.value = borrowers;
  } catch (cause) {
    if (token === loadToken) {
      $q.notify({ type: "negative", message: messageOf(cause) });
    }
  }
}

/** 依輸入本地即時過濾既有借用人（大小寫無關、子字串）。 */
function filterBorrowers(
  input: string,
  update: (callback: () => void) => void
) {
  const needle = input.toLowerCase();
  update(() => {
    borrowerOptions.value = allBorrowers.value.filter(name =>
      name.toLowerCase().includes(needle)
    );
  });
}

/**
 * 自由輸入同步：q-select 的 `input-debounce` 只延後 `filter` 與
 * `update:input-value`，未選候選、未按 Enter 直接送出時仍須取得目前文字
 * （比照 AssignIpDialog 的位址欄寫法）。
 */
function onBorrowerInput(event: Event) {
  const target = event.target;
  if (target instanceof HTMLInputElement) {
    borrower.value = target.value;
  }
}

function borrowerRule(value: string | null) {
  return (value ?? "").trim() !== "" || "借用人為必填";
}

async function submit() {
  const name = borrower.value.trim();
  if (name === "") {
    $q.notify({ type: "negative", message: "借用人為必填" });
    return;
  }

  saving.value = true;
  try {
    await createLending(props.asset.id, {
      borrower: name,
      due_at: textOrNull(dueAt.value),
      note: textOrNull(note.value)
    });
    $q.notify({ type: "positive", message: "已借出" });
    emit("saved");
    open.value = false;
  } catch (cause) {
    $q.notify({ type: "negative", message: messageOf(cause) });
  } finally {
    saving.value = false;
  }
}

function textOrNull(value: string): string | null {
  const text = value.trim();
  return text === "" ? null : text;
}

function messageOf(cause: unknown): string {
  return cause instanceof Error ? cause.message : String(cause);
}
</script>
