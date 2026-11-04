<script setup lang="ts">
// Extract dialog: whole archive or a selected subset; destination + same-name folder + advanced options.
import { computed, nextTick, ref, watch } from "vue";
import AppIcon from "./AppIcon.vue";
import FileIcon from "./FileIcon.vue";
import { fmtBytes, pickFolder } from "../lib/backend";
import { t } from "../lib/i18n";
import { onEsc } from "../lib/esc";
import { validateExtractOptions } from "../lib/validation";

const props = defineProps<{
  archivePath: string;
  archiveName: string;
  /** null = whole archive; otherwise the selected file/directory paths */
  selection: string[] | null;
  stats: { files: number; bytes: number };
  defaultDest: string;
  defaultCreateFolder?: boolean;
  password?: string;
}>();

const emit = defineEmits<{
  start: [args: { dest: string; entries: string[] | undefined; conflict: string; encoding: string; password?: string }];
  close: [];
}>();

const stem = computed(() => props.archiveName.replace(/\.[^.\\/]+$/, "") || props.archiveName);

const createFolder = ref(props.defaultCreateFolder ?? true);
const dest = ref(withFolder(props.defaultDest, createFolder.value));
function withFolder(base: string, on: boolean): string {
  const clean = base.replace(/[\\/]+$/, "");
  return on ? `${clean}/${stem.value}` : clean;
}
watch(createFolder, (on) => {
  dest.value = withFolder(props.defaultDest, on);
});

// Persist whether the advanced section is expanded
const advOpen = ref(localStorage.getItem("wispzip.extract.advOpen") === "1");
const conflict = ref("rename");
const encoding = ref("auto");
const password = ref(props.password ?? "");
const validationError = ref<string | null>(null);

watch(advOpen, (v) => {
  localStorage.setItem("wispzip.extract.advOpen", v ? "1" : "0");
});

watch(
  () => props.password,
  (p) => { if (p) password.value = p; },
);

// Live validation: clear the error when the path changes
watch([dest, password], () => {
  if (validationError.value) validationError.value = null;
});

// Destination input ref, used for autofocus
const destInputRef = ref<HTMLInputElement | null>(null);

// Autofocus the destination input
nextTick(() => {
  destInputRef.value?.focus();
  destInputRef.value?.select();
});

async function browse() {
  const picked = await pickFolder();
  if (picked) dest.value = picked;
}

function start() {
  validationError.value = null;
  try {
    validateExtractOptions({
      dest: dest.value.replace(/[\\/]+$/, ""),
      entries: props.selection ? props.selection : undefined,
      conflict: conflict.value,
      encoding: encoding.value,
      password: password.value || undefined,
    });
    emit("start", {
      dest: dest.value.replace(/[\\/]+$/, ""),
      entries: props.selection ? props.selection : undefined,
      conflict: conflict.value,
      encoding: encoding.value,
      password: password.value || undefined,
    });
  } catch (e) {
    if (e instanceof Error) {
      validationError.value = e.message;
    }
  }
}

// Close the dialog on Esc
onEsc(() => emit("close"));
</script>

<template>
  <div class="mask" role="dialog" aria-modal="true" :aria-label="t('extract.title', { name: archiveName })" @click.self="emit('close')">
    <div class="dialog">
      <header class="dlg-head">
        <FileIcon :name="archiveName" :is-dir="false" :size="34" aria-hidden="true" />
        <div class="dlg-titles">
          <div class="dlg-title">{{ t("extract.title", { name: archiveName }) }}</div>
          <div class="dlg-sub">
            <template v-if="selection">{{ t("extract.selectedFiles", { n: stats.files }) }} · {{ fmtBytes(stats.bytes) }}</template>
            <template v-else>{{ t("browser.nFiles", { n: stats.files }) }} · {{ fmtBytes(stats.bytes) }}</template>
          </div>
        </div>
        <button class="dlg-close" :aria-label="t('common.close')" @click="emit('close')"><AppIcon name="close" :size="15" /></button>
      </header>

      <label class="field">
        <span>{{ t("extract.destLabel") }}</span>
        <div class="row">
          <div class="dest" :class="{ error: validationError }">
            <AppIcon name="folder" :size="15" class="dest-ico" aria-hidden="true" />
            <input ref="destInputRef" v-model="dest" spellcheck="false" :aria-label="t('extract.destLabel')" :aria-invalid="!!validationError" />
          </div>
          <button class="ghost" @click="browse">{{ t("common.browse") }}</button>
        </div>
      </label>

      <label class="check-row">
        <input v-model="createFolder" type="checkbox" />
        <span>{{ t("extract.createFolder", { stem }) }}</span>
      </label>

      <button class="adv-toggle" :aria-expanded="advOpen" @click="advOpen = !advOpen">
        <AppIcon :name="advOpen ? 'chevron-down' : 'chevron-right'" :size="13" aria-hidden="true" />
        {{ t("compress.advanced") }}
      </button>
      <div v-if="advOpen" class="adv">
        <label><span>{{ t("extract.conflict") }}</span>
          <select v-model="conflict">
            <option value="rename">{{ t("extract.rename") }}</option>
            <option value="overwrite">{{ t("extract.overwrite") }}</option>
            <option value="skip">{{ t("extract.skip") }}</option>
          </select>
        </label>
        <label><span>{{ t("extract.encoding") }}</span>
          <select v-model="encoding">
            <option value="auto">{{ t("extract.encAuto") }}</option>
            <option value="utf8">UTF-8</option>
            <option value="gbk">{{ t("extract.encGbk") }}</option>
          </select>
        </label>
        <label><span>{{ t("extract.password") }}</span>
          <input v-model="password" type="password" :placeholder="t('extract.pwHint')" />
        </label>
      </div>

      <footer class="dlg-actions">
        <div v-if="validationError" class="error-msg" role="alert">{{ validationError }}</div>
        <button class="ghost" @click="emit('close')">{{ t("common.cancel") }}</button>
        <button class="primary" @click="start">
          {{ selection && stats.files ? t("extract.startN", { n: stats.files }) : t("extract.start") }}
        </button>
      </footer>
    </div>
  </div>
</template>

<style scoped>
.mask {
  position: fixed;
  inset: 0;
  z-index: 60;
  background: var(--mask);
  backdrop-filter: blur(4px);
  display: flex;
  align-items: center;
  justify-content: center;
  animation: fade 0.14s ease;
}
@keyframes fade { from { opacity: 0; } }

.dialog {
  width: min(440px, 92vw);
  background: var(--panel);
  border: 1px solid var(--border);
  border-radius: 16px;
  box-shadow: var(--shadow-pop);
  padding: 20px;
  display: flex;
  flex-direction: column;
  gap: 14px;
  animation: pop 0.16s cubic-bezier(0.2, 0.9, 0.3, 1.2);
}
@keyframes pop { from { opacity: 0; transform: scale(0.96) translateY(6px); } }

.dlg-head { display: flex; align-items: center; gap: 12px; }
.dlg-titles { flex: 1; min-width: 0; }
.dlg-title { font-size: 15px; font-weight: 700; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.dlg-sub { font-size: 12.5px; color: var(--text-dim); margin-top: 1px; }
.dlg-close {
  border: none;
  background: transparent;
  color: var(--text-dim);
  width: 28px;
  height: 28px;
  border-radius: 8px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
}
.dlg-close:hover { background: var(--bg-hover); color: var(--text); }

.field { display: flex; flex-direction: column; gap: 6px; font-size: 13px; }
.field > span { color: var(--text-dim); font-size: 12.5px; }
.row { display: flex; gap: 8px; align-items: center; }
.dest {
  flex: 1;
  display: flex;
  align-items: center;
  gap: 7px;
  border: 1px solid var(--border);
  border-radius: 9px;
  padding: 0 10px;
  background: var(--bg-soft);
  min-width: 0;
  transition: border-color 0.15s;
}
.dest:focus-within { border-color: var(--accent); }
.dest.error { border-color: var(--red); }
.dest.error:focus-within { border-color: var(--red); }
.dest-ico { color: var(--t-folder); flex: none; }
.dest input { border: none; background: transparent; padding: 10px 0; font-size: 12.5px; width: 100%; }

.check-row {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 13px;
  cursor: pointer;
  user-select: none;
}

.adv-toggle {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  align-self: flex-start;
  border: none;
  background: transparent;
  color: var(--text-dim);
  font-size: 12.5px;
  cursor: pointer;
  padding: 2px 0;
}
.adv-toggle:hover { color: var(--accent); }
.adv {
  display: flex;
  flex-direction: column;
  gap: 10px;
  background: var(--bg-soft);
  border: 1px solid var(--border);
  border-radius: 12px;
  padding: 12px;
}
.adv label { display: flex; flex-direction: column; gap: 4px; font-size: 12.5px; }
.adv label > span { color: var(--text-dim); }

.dlg-actions { display: flex; gap: 10px; justify-content: flex-end; align-items: center; }
.error-msg { color: var(--red, #e53e3e); font-size: 12px; margin-right: auto; }
</style>
