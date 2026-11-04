<script setup lang="ts">
// Settings: UI language, default format/level, hidden files, macOS Finder services installation.
import { onBeforeUnmount, onMounted, ref } from "vue";
import { installFinderServices, isTauri } from "../lib/backend";
import { t, setLocale, getLocaleSetting, type LocaleSetting } from "../lib/i18n";
import { onEsc } from "../lib/esc";

const props = defineProps<{
  defFormat: string;
  defPreset: string;
  defExcludeHidden: boolean;
}>();

const emit = defineEmits<{
  save: [v: { defFormat: string; defPreset: string; defExcludeHidden: boolean }];
  close: [];
}>();

const isMac = /Mac/i.test(navigator.userAgent);

const defFormat = ref(props.defFormat);
const defPreset = ref(props.defPreset);
const defExcludeHidden = ref(props.defExcludeHidden);
const svcInstalling = ref(false);
const svcDone = ref(false);

// UI language: applies immediately and persists
const lang = ref<LocaleSetting>(getLocaleSetting());
function setLang(v: LocaleSetting) {
  lang.value = v;
  setLocale(v);
}

const FORMATS = [
  { id: "zip", label: () => t("settings.defFormatZip") },
  { id: "7z", label: () => t("settings.defFormat7z") },
  { id: "tar.gz", label: () => t("settings.defFormatTgz") },
  { id: "tar.zst", label: () => t("settings.defFormatTzst") },
];

// Close the dialog on Esc (also while an input is focused)
function onKey(e: KeyboardEvent) {
  if (e.key === "Escape") {
    e.stopPropagation();
    emit("close");
  }
}
onMounted(() => document.addEventListener("keydown", onKey, true));
onBeforeUnmount(() => document.removeEventListener("keydown", onKey, true));

async function installServices() {
  svcInstalling.value = true;
  svcDone.value = false;
  try {
    await installFinderServices();
    svcDone.value = true;
  } catch (e) {
    console.error("install_finder_services failed:", e);
  } finally {
    svcInstalling.value = false;
  }
}
</script>

<template>
  <div class="mask" @click.self="emit('close')">
    <div class="dialog">
      <div class="dlg-title">{{ t("settings.title") }}</div>

      <div class="field"><span>{{ t("settings.language") }}</span>
        <div class="seg">
          <button :class="{ on: lang === 'system' }" @click="setLang('system')">{{ t("settings.langSystem") }}</button>
          <button :class="{ on: lang === 'zh' }" @click="setLang('zh')">中文</button>
          <button :class="{ on: lang === 'en' }" @click="setLang('en')">English</button>
        </div>
      </div>

      <label class="field"><span>{{ t("settings.defFormat") }}</span>
        <select v-model="defFormat">
          <option v-for="f in FORMATS" :key="f.id" :value="f.id">{{ f.label() }}</option>
        </select>
      </label>

      <div class="field"><span>{{ t("settings.defSpeed") }}</span>
        <div class="seg">
          <button :class="{ on: defPreset === 'fast' }" @click="defPreset = 'fast'">{{ t("compress.speedFast") }}</button>
          <button :class="{ on: defPreset === 'std' }" @click="defPreset = 'std'">{{ t("compress.speedStd") }}</button>
          <button :class="{ on: defPreset === 'max' }" @click="defPreset = 'max'">{{ t("compress.speedMax") }}</button>
        </div>
      </div>

      <label class="check-row">
        <input v-model="defExcludeHidden" type="checkbox" />
        <span>{{ t("settings.defExclude") }}</span>
      </label>

      <template v-if="isMac && isTauri">
        <div class="field"><span>{{ t("settings.finderServices") }}</span>
          <button class="ghost" :disabled="svcInstalling" @click="installServices">
            {{ svcInstalling ? "…" : t("settings.installServices") }}
          </button>
          <div v-if="svcDone" class="hint">{{ t("settings.servicesHint") }}</div>
        </div>
      </template>

      <footer class="dlg-actions">
        <button class="ghost" @click="emit('close')">{{ t("common.cancel") }}</button>
        <button
          class="primary"
          @click="emit('save', { defFormat, defPreset, defExcludeHidden })"
        >{{ t("common.save") }}</button>
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
}
.dialog {
  width: min(380px, 92vw);
  background: var(--panel);
  border: 1px solid var(--border);
  border-radius: 16px;
  box-shadow: var(--shadow-pop);
  padding: 20px;
  display: flex;
  flex-direction: column;
  gap: 14px;
}
.dlg-title { font-size: 15px; font-weight: 700; }
.field { display: flex; flex-direction: column; gap: 6px; font-size: 13px; }
.field > span { color: var(--text-dim); font-size: 12.5px; }
.seg { display: flex; gap: 6px; }
.seg button {
  flex: 1;
  padding: 7px 0;
  border: 1px solid var(--border);
  background: var(--panel);
  border-radius: 9px;
  font-size: 12.5px;
  color: var(--text-dim);
  cursor: pointer;
}
.seg button.on { background: var(--accent-soft); color: var(--accent); border-color: var(--accent); }
.check-row { display: flex; align-items: center; gap: 8px; font-size: 13px; cursor: pointer; user-select: none; }
.hint { font-size: 11.5px; color: var(--text-dim); line-height: 1.5; }
.dlg-actions { display: flex; gap: 10px; justify-content: flex-end; }
</style>
