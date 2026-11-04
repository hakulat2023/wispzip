<script setup lang="ts">
// Job progress (ring) and the done page, shared by compress and extract.
import { computed, ref, watch } from "vue";
import AppIcon from "./AppIcon.vue";
import { fmtBytes } from "../lib/backend";
import { t } from "../lib/i18n";
import type { ProgressEvent } from "../lib/backend";

const props = defineProps<{
  title: string;
  subject: string;
  progress: ProgressEvent | null;
  paused: boolean;
}>();

const emit = defineEmits<{ togglePause: []; cancel: [] }>();

const R = 56;
const C = 2 * Math.PI * R;

// startedAt is initialized on the first progress event, not at module load
const startedAt = ref<number | null>(null);
const lastProcessed = ref(0);
const lastTime = ref(0);

watch(() => props.progress, (p) => {
  if (p && startedAt.value === null) {
    startedAt.value = Date.now() / 1000;
    lastProcessed.value = p.processedBytes;
    lastTime.value = startedAt.value;
  }
});

const frac = computed(() => {
  const p = props.progress;
  if (!p?.totalBytes) return null;
  return Math.min(1, p.processedBytes / p.totalBytes);
});
const pctText = computed(() => (frac.value == null ? null : Math.round(frac.value * 100)));

// Speed: instantaneous speed from the last two progress samples, not a cumulative average
const speed = computed(() => {
  const p = props.progress;
  if (!p || props.paused || startedAt.value === null) return "";
  const now = Date.now() / 1000;
  const timeDelta = now - lastTime.value;
  if (timeDelta < 0.3) {
    // Sample interval too short: keep the existing speed (no update)
    return speedBps.value;
  }
  const bytesDelta = p.processedBytes - lastProcessed.value;
  lastProcessed.value = p.processedBytes;
  lastTime.value = now;
  return bytesDelta / timeDelta;
});
const speedBps = ref(0);
watch(speed, (v) => {
  if (typeof v === "number" && v > 0) speedBps.value = v;
});

const speedText = computed(() => {
  if (!speedBps.value || props.paused) return "";
  return `${fmtBytes(speedBps.value)}/s`;
});

const eta = computed(() => {
  if (props.paused) return null;
  const p = props.progress;
  if (!p?.totalBytes || frac.value == null || frac.value <= 0.01 || !speedBps.value) return null;
  const remain = (p.totalBytes - p.processedBytes) / speedBps.value;
  if (!isFinite(remain) || remain < 0) return null;
  return remain < 90 ? t("progress.etaSec", { n: Math.ceil(remain) }) : t("progress.etaMin", { n: Math.ceil(remain / 60) });
});
</script>

<template>
  <main class="prog" role="main" :aria-label="title">
    <div class="ring-wrap">
      <svg class="ring" :class="{ spin: frac == null, paused }" width="140" height="140" viewBox="0 0 140 140" aria-hidden="true">
        <circle cx="70" cy="70" :r="R" fill="none" stroke="var(--border)" stroke-width="9" />
        <circle
          cx="70"
          cy="70"
          :r="R"
          fill="none"
          stroke="var(--accent)"
          stroke-width="9"
          stroke-linecap="round"
          :stroke-dasharray="C"
          :stroke-dashoffset="frac == null ? C * 0.75 : C * (1 - frac)"
          transform="rotate(-90 70 70)"
        />
      </svg>
      <div class="ring-center">
        <template v-if="paused"><AppIcon name="pause" :size="22" /></template>
        <template v-else-if="pctText != null"><span class="pct">{{ pctText }}<small>%</small></span></template>
        <template v-else><span class="pct-dots">···</span></template>
      </div>
    </div>

    <div class="prog-title">{{ paused ? t("progress.paused") : props.title }}</div>
    <div class="prog-subject">{{ subject }}</div>

    <div class="prog-meta" aria-live="polite">
      <template v-if="progress?.totalBytes">{{ fmtBytes(progress.processedBytes) }} / {{ fmtBytes(progress.totalBytes) }}</template>
      <template v-if="progress?.filesDone"> · {{ progress.filesTotal ? t("progress.filesOfTotal", { a: progress.filesDone, b: progress.filesTotal }) : t("progress.filesOf", { n: progress.filesDone }) }}</template>
    </div>
    <div class="prog-speed" aria-live="polite">{{ speedText }}</div>
    <div class="prog-eta" aria-live="polite">{{ eta ?? "" }}</div>

    <div class="prog-actions">
      <button class="ghost" :aria-label="paused ? t('progress.resume') : t('progress.pause')" @click="emit('togglePause')">
        <AppIcon :name="paused ? 'play' : 'pause'" :size="14" />
        {{ paused ? t("progress.resume") : t("progress.pause") }}
      </button>
      <button class="ghost danger" :aria-label="t('common.cancel')" @click="emit('cancel')">{{ t("common.cancel") }}</button>
    </div>
  </main>
</template>

<style scoped>
.prog {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 6px;
  padding: 20px;
  text-align: center;
}
.ring-wrap { position: relative; width: 140px; height: 140px; margin-bottom: 10px; }
.ring { display: block; }
.ring.spin circle:last-child { animation: dash 1.2s ease-in-out infinite; }
.ring.paused circle:last-child { stroke: var(--amber); }
@keyframes dash {
  0% { stroke-dashoffset: calc(2 * 3.14159 * 56 * 0.75); }
  50% { stroke-dashoffset: calc(2 * 3.14159 * 56 * 0.55); }
  100% { stroke-dashoffset: calc(2 * 3.14159 * 56 * 0.75); }
}
.ring-center {
  position: absolute;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--text);
}
.pct { font-size: 26px; font-weight: 800; font-variant-numeric: tabular-nums; }
.pct small { font-size: 14px; font-weight: 600; color: var(--text-dim); }
.pct-dots { font-size: 24px; letter-spacing: 4px; color: var(--text-dim); }

.prog-title { font-size: 17px; font-weight: 700; }
.prog-subject { font-size: 13px; color: var(--text-dim); max-width: 80%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.prog-meta { font-size: 12.5px; color: var(--text-dim); font-variant-numeric: tabular-nums; margin-top: 8px; }
.prog-speed { font-size: 12px; color: var(--accent); font-weight: 600; min-height: 18px; font-variant-numeric: tabular-nums; }
.prog-eta { font-size: 12px; color: var(--text-dim); min-height: 18px; }

.prog-actions { display: flex; gap: 10px; margin-top: 14px; }
.ghost { display: inline-flex; align-items: center; gap: 6px; }
.ghost.danger { color: var(--red); border-color: transparent; }
.ghost.danger:hover { background: color-mix(in srgb, var(--red) 12%, transparent); }
</style>
