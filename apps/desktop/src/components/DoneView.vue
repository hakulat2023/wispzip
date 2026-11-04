<script setup lang="ts">
// Done page: result summary + reveal in the file manager + keep processing other files.
import AppIcon from "./AppIcon.vue";
import { t } from "../lib/i18n";

const props = defineProps<{
  title: string;
  subject: string;
  detail?: string | null;
}>();
const emit = defineEmits<{ reveal: []; done: []; continue: [] }>();
</script>

<template>
  <main class="done">
    <div class="done-ring"><AppIcon name="check" :size="34" /></div>
    <div class="done-title" aria-live="polite">{{ props.title }}</div>
    <div class="done-subject">{{ props.subject }}</div>
    <div v-if="props.detail" class="done-detail">{{ props.detail }}</div>
    <div class="done-actions">
      <button class="primary" @click="emit('reveal')"><AppIcon name="reveal" :size="15" /> {{ t("done.reveal") }}</button>
      <button class="ghost" @click="emit('continue')">{{ t("done.continue") }}</button>
      <button class="ghost" @click="emit('done')">{{ t("done.finish") }}</button>
    </div>
  </main>
</template>

<style scoped>
.done {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 8px;
  padding: 20px;
  text-align: center;
}
.done-ring {
  width: 84px;
  height: 84px;
  border-radius: 50%;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--green);
  background: color-mix(in srgb, var(--green) 14%, transparent);
  border: 2.5px solid color-mix(in srgb, var(--green) 45%, transparent);
  margin-bottom: 10px;
  animation: pop 0.24s cubic-bezier(0.2, 0.9, 0.3, 1.4);
}
@keyframes pop { from { opacity: 0; transform: scale(0.7); } }
.done-title { font-size: 17px; font-weight: 700; }
.done-subject { font-size: 13.5px; font-weight: 600; max-width: 82%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.done-detail { font-size: 12.5px; color: var(--text-dim); }
.done-actions { display: flex; gap: 10px; margin-top: 18px; }
</style>
