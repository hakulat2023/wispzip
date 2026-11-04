<script setup lang="ts">
// Password gate component: the unlock screen shown when an archive requires a password.

import { ref } from "vue";
import AppIcon from "./AppIcon.vue";
import { t } from "../lib/i18n";

const props = defineProps<{
  archiveName: string;
}>();

const emit = defineEmits<{
  unlock: [password: string];
  cancel: [];
}>();

const pw = ref("");

function submitPw() {
  if (pw.value.trim()) emit("unlock", pw.value);
}
</script>

<template>
  <div class="gate">
    <div class="gate-card">
      <div class="gate-ico"><AppIcon name="lock" :size="22" /></div>
      <div class="gate-title">{{ t("browser.gateTitle") }}</div>
      <div class="gate-hint">{{ t("browser.gateHint") }}</div>
      <input
        v-model="pw"
        class="gate-input"
        type="password"
        :placeholder="t('browser.gatePlaceholder')"
        autofocus
        aria-label="压缩包密码"
        @keyup.enter="submitPw"
      />
      <div class="gate-actions">
        <button class="ghost" aria-label="取消" @click="emit('cancel')">{{ t("common.cancel") }}</button>
        <button class="primary" aria-label="解锁" @click="submitPw">{{ t("browser.unlock") }}</button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.gate { flex: 1; display: flex; align-items: center; justify-content: center; }
.gate-card {
  width: min(320px, 86%);
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 8px;
  padding: 30px 28px;
  border: 1px solid var(--border);
  border-radius: 16px;
  background: var(--panel);
  box-shadow: var(--shadow-card);
}
.gate-ico {
  width: 52px;
  height: 52px;
  border-radius: 16px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--accent-soft);
  color: var(--accent);
  margin-bottom: 4px;
}
.gate-title { font-size: 15px; font-weight: 700; }
.gate-hint { font-size: 12.5px; color: var(--text-dim); }
.gate-input { width: 100%; margin-top: 8px; }
.gate-actions { display: flex; gap: 10px; margin-top: 12px; width: 100%; }
.gate-actions .ghost, .gate-actions .primary { flex: 1; }
</style>
