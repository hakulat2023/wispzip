<script setup lang="ts">
// Compress confirmation page: output name, format, level, password/split and advanced options.
import { computed, ref } from "vue";
import AppIcon from "./AppIcon.vue";
import FileIcon from "./FileIcon.vue";
import { fmtBytes, pickSave } from "../lib/backend";
import { t } from "../lib/i18n";
import type { ScanStats } from "../lib/backend";

const props = defineProps<{
  inputs: string[];
  scan: ScanStats | null;
  scanning: boolean;
  defaultOut: string;
  defaultFormat: string;
  defaultPreset: string;
  defaultExcludeHidden: boolean;
}>();

const emit = defineEmits<{
  start: [args: { out: string; level: number; password?: string; splitBytes?: number; excludeHidden: boolean }];
  close: [];
}>();

const FORMAT_DEFS = [
  { id: "zip", hintKey: "compress.fmtGeneric" },
  { id: "7z", hintKey: "compress.fmtBest" },
  { id: "tar.gz", hintKey: "compress.fmtCompat" },
  { id: "tar.zst", hintKey: "compress.fmtFast" },
];
const LEVEL_PRESETS: Record<string, Record<string, number>> = {
  zip: { fast: 1, std: 6, max: 9 },
  "7z": { fast: 1, std: 6, max: 9 },
  "tar.gz": { fast: 1, std: 6, max: 9 },
  "tar.zst": { fast: 3, std: 10, max: 19 },
};

const out = ref(props.defaultOut);
const format = ref(props.defaultFormat);
const levelPreset = ref(props.defaultPreset as "fast" | "std" | "max");
const usePassword = ref(false);
const password = ref("");
const useSplit = ref(false);
const splitSize = ref("100M");
const excludeHidden = ref(props.defaultExcludeHidden);
const advOpen = ref(false);
const error = ref("");

const is7zish = computed(() => format.value === "zip" || format.value === "7z");
const subject = computed(() => props.inputs[0]?.split(/[\\/]/).pop() ?? "");

// Password strength estimation
const passwordStrength = computed(() => {
  const pw = password.value;
  if (!pw) return { level: 0, label: "", color: "" };

  let score = 0;
  if (pw.length >= 8) score++;
  if (pw.length >= 12) score++;
  if (/[a-z]/.test(pw) && /[A-Z]/.test(pw)) score++;
  if (/\d/.test(pw)) score++;
  if (/[^a-zA-Z0-9]/.test(pw)) score++;

  if (score <= 1) return { level: 1, label: t("compress.pwWeak"), color: "var(--red)" };
  if (score <= 2) return { level: 2, label: t("compress.pwFair"), color: "var(--amber)" };
  if (score <= 3) return { level: 3, label: t("compress.pwGood"), color: "var(--accent)" };
  return { level: 4, label: t("compress.pwStrong"), color: "var(--green)" };
});

function applyFormat(id: string) {
  format.value = id;
  const ext = id === "tar.gz" ? "tar.gz" : id;
  const stem = out.value.replace(/\.(zip|7z|tar\.gz|tar\.zst)$/, "");
  out.value = `${stem}.${ext}`;
}
function applyPreset(p: "fast" | "std" | "max") {
  levelPreset.value = p;
}

function parseSplitSize(input: string): number | undefined {
  const s = input.trim().toUpperCase().replace(/B$/, "");
  if (!s) return undefined;
  const m = s.match(/^(\d+(?:\.\d+)?)\s*([KMGT]?)$/);
  if (!m) throw t("compress.errSplitFormat");
  const mult: Record<string, number> = { "": 1, K: 1024, M: 1024 ** 2, G: 1024 ** 3, T: 1024 ** 4 };
  return Math.round(parseFloat(m[1]) * mult[m[2]]);
}

async function browseOut() {
  const name = out.value.split(/[\\/]/).pop() || "archive.zip";
  const picked = await pickSave(name);
  if (picked) out.value = picked;
}

function start() {
  error.value = "";
  try {
    let splitBytes: number | undefined;
    if (useSplit.value) {
      splitBytes = parseSplitSize(splitSize.value);
      if (splitBytes !== undefined && splitBytes < 1024) throw t("compress.errSplitMin");
    }
    emit("start", {
      out: out.value,
      level: LEVEL_PRESETS[format.value]?.[levelPreset.value] ?? 6,
      password: is7zish.value && usePassword.value ? password.value : undefined,
      splitBytes,
      excludeHidden: excludeHidden.value,
    });
  } catch (e) {
    error.value = String(e);
  }
}
</script>

<template>
  <main class="sheet">
    <button class="back" @click="emit('close')">
      <AppIcon name="arrow-left" :size="15" /> {{ t("common.back") }}
    </button>

    <div class="hero">
      <FileIcon :name="subject" :is-dir="false" :size="52" />
      <div class="hero-name">
        {{ subject }}<template v-if="inputs.length > 1"> {{ t("compress.andN", { n: inputs.length }) }}</template>
      </div>
      <div class="hero-sub">
        <template v-if="scan">{{ t("compress.filesAndSize", { n: scan.files, size: fmtBytes(scan.bytes) }) }}</template>
        <template v-else-if="scanning">{{ t("compress.scanning") }}</template>
      </div>
    </div>

    <label class="field">
      <span>{{ t("compress.saveAs") }}</span>
      <div class="row">
        <div class="dest"><AppIcon name="archive" :size="15" class="dest-ico" /><input v-model="out" spellcheck="false" /></div>
        <button class="ghost" @click="browseOut">{{ t("common.browse") }}</button>
      </div>
    </label>

    <div class="seg-row">
      <span class="seg-label">{{ t("compress.format") }}</span>
      <div class="seg">
        <button
          v-for="f in FORMAT_DEFS"
          :key="f.id"
          :class="{ on: format === f.id }"
          @click="applyFormat(f.id)"
        >
          {{ f.id === "tar.gz" ? "TAR.GZ" : f.id === "tar.zst" ? "ZST" : f.id.toUpperCase() }}<small>{{ t(f.hintKey) }}</small>
        </button>
      </div>
    </div>
    <div class="seg-row">
      <span class="seg-label">{{ t("compress.speed") }}</span>
      <div class="seg">
        <button :class="{ on: levelPreset === 'fast' }" @click="applyPreset('fast')">{{ t("compress.speedFast") }}</button>
        <button :class="{ on: levelPreset === 'std' }" @click="applyPreset('std')">{{ t("compress.speedStd") }}</button>
        <button :class="{ on: levelPreset === 'max' }" @click="applyPreset('max')">{{ t("compress.speedMax") }}</button>
      </div>
    </div>

    <button class="adv-toggle" @click="advOpen = !advOpen">
      <AppIcon :name="advOpen ? 'chevron-down' : 'chevron-right'" :size="13" />
      {{ t("compress.advanced") }}
    </button>
    <div v-if="advOpen" class="adv">
      <label v-if="is7zish" class="check-row">
        <input v-model="usePassword" type="checkbox" />
        <span>{{ format === "zip" ? t("compress.setPasswordZip") : t("compress.setPassword") }}</span>
        <input v-if="usePassword" v-model="password" type="password" :placeholder="t('compress.pwPlaceholder')" class="pw" />
      </label>
      <div v-if="usePassword && password" class="pw-strength">
        <div class="pw-strength-bar">
          <div
            class="pw-strength-fill"
            :style="{ width: `${(passwordStrength.level / 4) * 100}%`, backgroundColor: passwordStrength.color }"
          />
        </div>
        <span class="pw-strength-label" :style="{ color: passwordStrength.color }">{{ passwordStrength.label }}</span>
      </div>
      <label class="check-row">
        <input v-model="useSplit" type="checkbox" />
        <span>{{ t("compress.split") }}</span>
        <input v-if="useSplit" v-model="splitSize" placeholder="100M / 1G" class="pw" />
      </label>
      <label class="check-row">
        <input v-model="excludeHidden" type="checkbox" />
        <span>{{ t("compress.excludeHidden") }}</span>
      </label>
    </div>

    <div v-if="error" class="error" role="alert">{{ error }}</div>

    <footer class="actions">
      <button class="ghost" @click="emit('close')">{{ t("common.cancel") }}</button>
      <button class="primary big" :disabled="!out" @click="start">{{ t("compress.start") }}</button>
    </footer>
  </main>
</template>

<style scoped>
.sheet {
  flex: 1;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 14px;
  padding: 8px 26px 22px;
  max-width: 520px;
  margin: 0 auto;
  width: 100%;
}
.back {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  align-self: flex-start;
  border: none;
  background: transparent;
  color: var(--text-dim);
  font-size: 13px;
  cursor: pointer;
  padding: 4px 0;
}
.back:hover { color: var(--text); }

.hero { display: flex; flex-direction: column; align-items: center; gap: 8px; text-align: center; padding: 6px 0; }
.hero-name { font-size: 16px; font-weight: 700; word-break: break-all; }
.hero-sub { font-size: 12.5px; color: var(--text-dim); }

.field { display: flex; flex-direction: column; gap: 6px; }
.field > span { font-size: 12.5px; color: var(--text-dim); }
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
}
.dest:focus-within { border-color: var(--accent); }
.dest-ico { color: var(--t-archive); flex: none; }
.dest input { border: none; background: transparent; padding: 8px 0; font-size: 12.5px; }

.seg-row { display: flex; align-items: center; gap: 12px; }
.seg-label { width: 34px; font-size: 12.5px; color: var(--text-dim); flex: none; }
.seg { display: flex; gap: 6px; flex: 1; }
.seg button {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 1px;
  padding: 8px 0;
  border: 1px solid var(--border);
  background: var(--panel);
  border-radius: 10px;
  font-size: 12.5px;
  font-weight: 600;
  color: var(--text-dim);
  cursor: pointer;
}
.seg button small { font-size: 10.5px; font-weight: 400; opacity: 0.75; }
.seg button.on { background: var(--accent-soft); color: var(--accent); border-color: var(--accent); }

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
  gap: 11px;
  background: var(--bg-soft);
  border: 1px solid var(--border);
  border-radius: 12px;
  padding: 13px;
}
.check-row { display: flex; align-items: center; gap: 8px; font-size: 13px; cursor: pointer; user-select: none; }
.pw {
  margin-left: auto;
  width: 150px;
  font-size: 12px;
  padding: 6px 8px;
  border-radius: 8px;
  border: 1px solid var(--border);
  background: var(--panel);
  color: var(--text);
  outline: none;
}
.pw:focus { border-color: var(--accent); }

.pw-strength {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-top: 6px;
  margin-left: 26px;
}
.pw-strength-bar {
  flex: 1;
  height: 4px;
  background: var(--border);
  border-radius: 2px;
  overflow: hidden;
}
.pw-strength-fill {
  height: 100%;
  transition: width 0.3s ease, background-color 0.3s ease;
}
.pw-strength-label {
  font-size: 11px;
  font-weight: 500;
  min-width: 40px;
}

.error { color: var(--red); font-size: 12.5px; }

.actions { display: flex; gap: 10px; justify-content: flex-end; margin-top: auto; padding-top: 4px; }
.big { padding: 10px 26px; font-size: 14px; }
</style>
