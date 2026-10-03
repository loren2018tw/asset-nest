<template>
  <q-dialog v-model="open" persistent>
    <q-card style="width: 640px; max-width: 95vw">
      <q-form @submit="submit">
        <q-card-section>
          <div class="text-h6">
            {{ asset === null ? "新增資產" : "編輯資產" }}
          </div>
        </q-card-section>

        <q-card-section class="q-gutter-y-sm">
          <q-banner
            v-if="errorMessage !== ''"
            dense
            rounded
            class="bg-negative text-white"
          >
            {{ errorMessage }}
          </q-banner>

          <q-input v-model="form.property_no" outlined dense label="財產編號" />

          <q-input
            v-model="form.description"
            outlined
            dense
            label="描述 *"
            :rules="[requiredRule('描述')]"
            lazy-rules
          />

          <div ref="locationField">
            <q-input
              v-model="form.location"
              outlined
              dense
              label="位置 *"
              :rules="[requiredRule('位置')]"
              lazy-rules
              @focus="locationMenuOpen = true"
              @update:model-value="locationMenuOpen = true"
            />
            <q-menu
              v-if="locationTarget !== undefined"
              v-model="locationMenuOpen"
              :target="locationTarget"
              no-parent-event
              no-focus
              no-refocus
              fit
              anchor="bottom left"
              self="top left"
            >
              <q-list dense>
                <q-item
                  v-for="suggestion in locationSuggestions"
                  :key="suggestion"
                  v-close-popup
                  clickable
                  @mousedown.prevent
                  @click="pickLocation(suggestion)"
                >
                  <q-item-section>{{ suggestion }}</q-item-section>
                </q-item>
                <q-item v-if="locationSuggestions.length === 0" disable>
                  <q-item-section class="text-grey-6">
                    沒有符合的既有位置
                  </q-item-section>
                </q-item>
              </q-list>
            </q-menu>
          </div>

          <q-input
            v-model="form.device_serial"
            outlined
            dense
            label="設備序號"
            hint="可搜尋；重複僅提示，不阻擋儲存"
          />

          <q-banner
            v-if="duplicateSerial"
            dense
            rounded
            class="bg-warning text-black"
          >
            已有其他資產使用此設備序號；仍可儲存。
          </q-banner>

          <div class="row q-col-gutter-sm">
            <div class="col-12 col-sm-6">
              <q-input v-model="form.brand" outlined dense label="廠牌" />
            </div>
            <div class="col-12 col-sm-6">
              <q-input v-model="form.model" outlined dense label="型號" />
            </div>
          </div>

          <div class="row q-col-gutter-sm">
            <div class="col-12 col-sm-6">
              <q-input
                v-model="form.purchase_date"
                outlined
                dense
                type="date"
                label="購置日期"
              />
            </div>
            <div class="col-12 col-sm-6">
              <q-input
                v-model="form.lifespan_years"
                outlined
                dense
                type="number"
                min="0"
                label="年限（年）"
                :rules="[lifespanRule]"
                lazy-rules
              />
            </div>
          </div>

          <q-input
            v-model="form.note"
            outlined
            dense
            type="textarea"
            autogrow
            label="備註"
          />
        </q-card-section>

        <q-card-actions align="right">
          <q-btn v-close-popup flat label="取消" />
          <q-btn color="primary" type="submit" label="儲存" :loading="saving" />
        </q-card-actions>
      </q-form>
    </q-card>
  </q-dialog>
</template>

<script setup lang="ts">
import { computed, ref, watch } from "vue";

import {
  createAsset,
  fetchLocations,
  findByDeviceSerial,
  updateAsset,
  type Asset,
  type AssetInput
} from "@/api/assets";

const props = defineProps<{
  modelValue: boolean;
  /** `null`＝新增；否則為編輯中的資產。 */
  asset: Asset | null;
}>();

const emit = defineEmits<{
  "update:modelValue": [value: boolean];
  saved: [];
}>();

interface FormState {
  property_no: string;
  description: string;
  location: string;
  device_serial: string;
  brand: string;
  model: string;
  purchase_date: string;
  lifespan_years: string;
  note: string;
}

function emptyForm(): FormState {
  return {
    property_no: "",
    description: "",
    location: "",
    device_serial: "",
    brand: "",
    model: "",
    purchase_date: "",
    lifespan_years: "",
    note: ""
  };
}

const form = ref<FormState>(emptyForm());
const saving = ref(false);
const errorMessage = ref("");
const allLocations = ref<string[]>([]);
const locationMenuOpen = ref(false);
const duplicateSerial = ref(false);
const locationField = ref<HTMLElement | null>(null);

/** QMenu 的定位目標；template ref 於掛載後才有值。 */
const locationTarget = computed(() => locationField.value ?? undefined);

/** 供 serial 重複檢查丟棄過期回應。 */
let serialCheckToken = 0;

const open = computed({
  get: () => props.modelValue,
  set: value => emit("update:modelValue", value)
});

const locationSuggestions = computed(() => {
  const needle = form.value.location.trim().toLowerCase();
  return allLocations.value.filter(location => {
    const candidate = location.toLowerCase();
    return (
      candidate !== needle && (needle === "" || candidate.includes(needle))
    );
  });
});

watch(
  () => props.modelValue,
  value => {
    if (value) {
      prepare();
    }
  }
);

watch(
  () => form.value.device_serial,
  serial => {
    const value = serial.trim();
    const token = ++serialCheckToken;
    duplicateSerial.value = false;

    if (value === "") {
      return;
    }

    window.setTimeout(() => {
      void checkDuplicateSerial(value, token);
    }, 400);
  }
);

function prepare() {
  const asset = props.asset;
  form.value =
    asset === null
      ? emptyForm()
      : {
          property_no: asset.property_no ?? "",
          description: asset.description,
          location: asset.location,
          device_serial: asset.device_serial ?? "",
          brand: asset.brand ?? "",
          model: asset.model ?? "",
          purchase_date: asset.purchase_date ?? "",
          lifespan_years:
            asset.lifespan_years === null ? "" : String(asset.lifespan_years),
          note: asset.note ?? ""
        };

  errorMessage.value = "";
  duplicateSerial.value = false;
  serialCheckToken += 1;
  void loadLocations();
}

async function loadLocations() {
  try {
    allLocations.value = await fetchLocations();
  } catch {
    // 建議值載入失敗不影響輸入與儲存
  }
}

async function checkDuplicateSerial(value: string, token: number) {
  try {
    const matches = await findByDeviceSerial(value);
    if (token === serialCheckToken) {
      duplicateSerial.value = matches.some(
        match => match.id !== props.asset?.id
      );
    }
  } catch {
    // 重複提示僅為輔助，查詢失敗不影響儲存
  }
}

function pickLocation(location: string) {
  form.value.location = location;
  locationMenuOpen.value = false;
}

function requiredRule(label: string) {
  return (value: string | null) =>
    (value ?? "").trim() !== "" || `${label}為必填`;
}

function lifespanRule(value: string | null) {
  const text = (value ?? "").trim();
  if (text === "") {
    return true;
  }

  const years = Number(text);
  return (Number.isInteger(years) && years >= 0) || "年限須為非負整數";
}

function textOrNull(value: string): string | null {
  const text = value.trim();
  return text === "" ? null : text;
}

function toInput(): AssetInput {
  const lifespan = form.value.lifespan_years.trim();
  return {
    property_no: textOrNull(form.value.property_no),
    description: form.value.description.trim(),
    location: form.value.location.trim(),
    device_serial: textOrNull(form.value.device_serial),
    brand: textOrNull(form.value.brand),
    model: textOrNull(form.value.model),
    purchase_date: textOrNull(form.value.purchase_date),
    lifespan_years: lifespan === "" ? null : Number(lifespan),
    note: textOrNull(form.value.note)
  };
}

async function submit() {
  saving.value = true;
  errorMessage.value = "";

  try {
    const input = toInput();
    if (props.asset === null) {
      await createAsset(input);
    } else {
      await updateAsset(props.asset.id, input);
    }

    emit("saved");
    open.value = false;
  } catch (cause) {
    errorMessage.value = cause instanceof Error ? cause.message : String(cause);
  } finally {
    saving.value = false;
  }
}
</script>
