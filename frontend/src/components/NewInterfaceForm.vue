<template>
  <div>
    <q-btn
      dense
      flat
      color="primary"
      icon="add"
      label="新增介面"
      :disable="disabled || saving"
      @click="showNewInterface = !showNewInterface"
    />
  </div>

  <q-card v-if="showNewInterface" flat bordered class="q-pa-sm">
    <q-banner
      v-if="newInterfaceError !== ''"
      dense
      rounded
      class="bg-negative text-white q-mb-sm"
    >
      {{ newInterfaceError }}
    </q-banner>
    <div class="row q-col-gutter-sm items-start">
      <div class="col-12 col-sm-5">
        <q-input
          v-model="newInterface.name"
          outlined
          dense
          label="名稱"
          :rules="[interfaceNameRule(newInterface.mac)]"
          lazy-rules
        />
      </div>
      <div class="col-12 col-sm-5">
        <q-input
          v-model="newInterface.mac"
          outlined
          dense
          label="MAC"
          :rules="[macRule]"
          lazy-rules
        />
      </div>
      <div class="col-12 col-sm-2 text-right">
        <q-btn
          dense
          flat
          color="primary"
          label="建立"
          :loading="saving"
          @click="createNewInterface"
        />
      </div>
    </div>
    <PeerMacHint class="q-mt-xs" @fill="fillNewInterfaceMac" />
  </q-card>
</template>

<script setup lang="ts">
import { useQuasar } from "quasar";
import { ref } from "vue";

import type { InterfaceSaved } from "@/api/interfaces";
import PeerMacHint from "@/components/PeerMacHint.vue";
import { createInterfaceFlow, interfaceNameRule } from "@/utils/assignmentFlow";
import { assignmentFlowDeps } from "@/utils/assignmentFlowDeps";
import { macRule } from "@/utils/mac";

/**
 * 當場新增介面（見 `.scratch/assignment-flow/spec.md` §決策 4）：兩對話框
 * 逐字相同的「新增介面」區（按鈕＋卡片＋`PeerMacHint`＋驗證＋錯誤 banner）；
 * 建立走 `createInterfaceFlow`（驗證與 warnings 提示在流程內），成功由
 * `created` 回報，父層負責加入清單並選取。
 */
const props = withDefaults(
  defineProps<{
    /** 目標資產 id；null（如未儲存資產）＝不建立。 */
    assetId: number | null;
    /** 停用「新增介面」按鈕（如網段端未選資產）。 */
    disabled?: boolean;
  }>(),
  { disabled: false }
);

const emit = defineEmits<{
  created: [saved: InterfaceSaved];
}>();

const $q = useQuasar();
const deps = assignmentFlowDeps($q);

const showNewInterface = ref(false);
const newInterface = ref({ name: "", mac: "" });
const saving = ref(false);
const newInterfaceError = ref("");

/** 將連線主機 MAC 填入當場新增介面的 MAC 欄位。 */
function fillNewInterfaceMac(mac: string) {
  newInterface.value.mac = mac;
}

/** 當場新增介面（名稱／MAC）；成功後收合卡片、重設欄位並回報父層。 */
async function createNewInterface() {
  const assetId = props.assetId;
  if (assetId === null) {
    return;
  }

  saving.value = true;
  newInterfaceError.value = "";
  try {
    const result = await createInterfaceFlow(deps, assetId, {
      name: newInterface.value.name,
      mac: newInterface.value.mac
    });
    if ("error" in result) {
      newInterfaceError.value = result.error;
      return;
    }
    emit("created", result.saved);
    showNewInterface.value = false;
    newInterface.value = { name: "", mac: "" };
  } finally {
    saving.value = false;
  }
}
</script>
