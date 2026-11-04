<script setup lang="ts">
// Built-in preview: text (monospace) and images; binaries fall back to open/extract.
import { computed, onMounted, ref } from "vue";
import AppIcon from "./AppIcon.vue";
import FileIcon from "./FileIcon.vue";
import { fmtBytes, previewEntry, openEntry, type PreviewResult } from "../lib/backend";
import { t } from "../lib/i18n";
import { onEsc } from "../lib/esc";

const props = defineProps<{
  archivePath: string;
  /** Full entry path (including the directory) */
  entryPath: string;
  encoding?: string;
  password?: string;
}>();

const emit = defineEmits<{
  extract: [entryPath: string];
  close: [];
}>();

const state = ref<"loading" | "text" | "image" | "binary" | "error">("loading");
const data = ref<PreviewResult | null>(null);
const errorMsg = ref("");
const opening = ref(false);

const name = computed(() => props.entryPath.split("/").pop() ?? props.entryPath);
const dir = computed(() => {
  const i = props.entryPath.lastIndexOf("/");
  return i < 0 ? props.archivePath.split(/[\\/]/).pop()! : `${props.archivePath.split(/[\\/]/).pop()!}/${props.entryPath.slice(0, i)}`;
});

// Image zoom/pan state
const zoom = ref(1);
const panX = ref(0);
const panY = ref(0);
const isPanning = ref(false);
let panStartX = 0;
let panStartY = 0;
let panStartPanX = 0;
let panStartPanY = 0;

const ZOOM_MIN = 0.1;
const ZOOM_MAX = 8;
const ZOOM_STEP = 0.25;

function zoomIn() {
  zoom.value = Math.min(ZOOM_MAX, zoom.value + ZOOM_STEP);
}
function zoomOut() {
  zoom.value = Math.max(ZOOM_MIN, zoom.value - ZOOM_STEP);
}
function zoomReset() {
  zoom.value = 1;
  panX.value = 0;
  panY.value = 0;
}
function zoomFit() {
  // Reset to fit (controlled by CSS)
  zoom.value = 0;
  panX.value = 0;
  panY.value = 0;
}

function onWheel(e: WheelEvent) {
  if (state.value !== "image") return;
  e.preventDefault();
  const delta = e.deltaY > 0 ? -ZOOM_STEP : ZOOM_STEP;
  const newZoom = Math.max(ZOOM_MIN, Math.min(ZOOM_MAX, (zoom.value || 1) + delta));
  // If currently in fit mode, switch to a custom zoom
  if (zoom.value === 0) zoom.value = 1;
  zoom.value = newZoom;
}

function onImageMouseDown(e: MouseEvent) {
  if (state.value !== "image" || zoom.value <= 1) return;
  isPanning.value = true;
  panStartX = e.clientX;
  panStartY = e.clientY;
  panStartPanX = panX.value;
  panStartPanY = panY.value;
  e.preventDefault();
}

function onMouseMove(e: MouseEvent) {
  if (!isPanning.value) return;
  panX.value = panStartPanX + (e.clientX - panStartX);
  panY.value = panStartPanY + (e.clientY - panStartY);
}

function onMouseUp() {
  isPanning.value = false;
}

function onImageDblClick() {
  if (zoom.value === 0 || zoom.value === 1) {
    zoom.value = 2;
  } else {
    zoomReset();
  }
}

const imageStyle = computed(() => {
  if (zoom.value === 0) {
    // fit mode
    return {
      transform: "",
      cursor: "zoom-in",
      maxWidth: "100%",
      maxHeight: "100%",
    };
  }
  return {
    transform: `translate(${panX.value}px, ${panY.value}px) scale(${zoom.value})`,
    cursor: zoom.value > 1 ? (isPanning.value ? "grabbing" : "grab") : "zoom-in",
    maxWidth: "none",
    maxHeight: "none",
  };
});

onMounted(async () => {
  try {
    const r = await previewEntry({
      archive: props.archivePath,
      entry: props.entryPath,
      encoding: props.encoding,
      password: props.password || undefined,
    });
    data.value = r;
    state.value = r.kind === "binary" ? "binary" : r.kind;
  } catch (e) {
    errorMsg.value = (e as { message?: string })?.message ?? String(e);
    state.value = "error";
  }

  // Global mouse event listeners
  document.addEventListener("mousemove", onMouseMove);
  document.addEventListener("mouseup", onMouseUp);
});

// Clean up listeners
import { onBeforeUnmount } from "vue";
onBeforeUnmount(() => {
  document.removeEventListener("mousemove", onMouseMove);
  document.removeEventListener("mouseup", onMouseUp);
});

async function openWithSystem() {
  opening.value = true;
  try {
    await openEntry({
      archive: props.archivePath,
      entry: props.entryPath,
      encoding: props.encoding,
      password: props.password || undefined,
    });
  } finally {
    opening.value = false;
  }
}

// Close the preview on Esc
onEsc(() => emit("close"));
</script>

<template>
  <div class="mask" role="dialog" :aria-label="t('preview.title')" @click.self="emit('close')">
    <div class="preview">
      <header class="pv-head">
        <button class="pv-back" :aria-label="t('common.close')" @click="emit('close')"><AppIcon name="arrow-left" :size="16" /></button>
        <div class="pv-path" :title="dir + ' / ' + name">
          <FileIcon :name="name" :is-dir="false" :size="20" aria-hidden="true" />
          <span class="pv-dir">{{ dir }} / </span>
          <span class="pv-name">{{ name }}</span>
        </div>
        <button class="pv-close" :aria-label="t('common.close')" @click="emit('close')"><AppIcon name="close" :size="15" /></button>
      </header>

      <div class="pv-body" @wheel="onWheel">
        <div v-if="state === 'loading'" class="pv-center" role="status" aria-live="polite">
          <div class="spinner" aria-hidden="true" />
        </div>

        <pre v-else-if="state === 'text' && data" class="pv-text">{{ data.content }}</pre>

        <div v-else-if="state === 'image' && data" class="pv-image">
          <img
            :src="data.content"
            :alt="name"
            :style="imageStyle"
            @mousedown="onImageMouseDown"
            @dblclick="onImageDblClick"
          />
        </div>

        <div v-else-if="state === 'binary'" class="pv-center">
          <FileIcon :name="name" :is-dir="false" :size="56" aria-hidden="true" />
          <div class="pv-empty-title">{{ t("preview.noPreview") }}</div>
          <div class="pv-empty-hint">{{ t("preview.binaryHint", { size: fmtBytes(data?.size ?? 0) }) }}</div>
          <button class="primary" :disabled="opening" @click="openWithSystem">
            {{ opening ? t("preview.opening") : t("preview.openWith") }}
          </button>
        </div>

        <div v-else class="pv-center" role="alert">
          <AppIcon name="info" :size="40" aria-hidden="true" />
          <div class="pv-empty-title">{{ t("preview.error") }}</div>
          <div class="pv-empty-hint">{{ errorMsg }}</div>
        </div>
      </div>

      <footer v-if="state === 'text' || state === 'image'" class="pv-foot">
        <span class="pv-size">
          {{ fmtBytes(data?.size ?? 0) }}
          <template v-if="data?.truncated"> · {{ t("preview.truncated", { size: fmtBytes(data.content.length) }) }}</template>
        </span>

        <!-- Image zoom controls -->
        <div v-if="state === 'image'" class="zoom-controls" role="toolbar" :aria-label="t('preview.zoom')">
          <button class="zoom-btn" :aria-label="t('preview.zoomOut')" :disabled="zoom <= ZOOM_MIN" @click="zoomOut">
            <AppIcon name="minus" :size="14" />
          </button>
          <span class="zoom-level" aria-live="polite">{{ zoom === 0 ? "Fit" : Math.round(zoom * 100) + "%" }}</span>
          <button class="zoom-btn" :aria-label="t('preview.zoomIn')" :disabled="zoom >= ZOOM_MAX" @click="zoomIn">
            <AppIcon name="plus" :size="14" />
          </button>
          <button class="zoom-btn" :aria-label="t('preview.zoomReset')" @click="zoomReset">
            <AppIcon name="refresh" :size="13" />
          </button>
          <button class="zoom-btn" :aria-label="t('preview.zoomFit')" @click="zoomFit">
            <AppIcon name="maximize" :size="13" />
          </button>
        </div>

        <span class="spacer" />
        <button class="ghost" @click="emit('extract', entryPath)">{{ t("common.extract") }}</button>
        <button class="primary" :disabled="opening" @click="openWithSystem">
          {{ opening ? t("preview.opening") : t("common.open") }}
        </button>
      </footer>
    </div>
  </div>
</template>

<style scoped>
.mask {
  position: fixed;
  inset: 0;
  z-index: 70;
  background: var(--mask);
  backdrop-filter: blur(6px);
  display: flex;
  align-items: center;
  justify-content: center;
  animation: fade 0.14s ease;
}
@keyframes fade { from { opacity: 0; } }

.preview {
  width: min(760px, 94vw);
  height: min(600px, 90vh);
  background: var(--panel);
  border: 1px solid var(--border);
  border-radius: 16px;
  box-shadow: var(--shadow-pop);
  display: flex;
  flex-direction: column;
  overflow: hidden;
  animation: pop 0.16s cubic-bezier(0.2, 0.9, 0.3, 1.2);
}
@keyframes pop { from { opacity: 0; transform: scale(0.97) translateY(8px); } }

.pv-head {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 10px 14px;
  border-bottom: 1px solid var(--border);
}
.pv-back, .pv-close {
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
  flex: none;
}
.pv-back:hover, .pv-close:hover { background: var(--bg-hover); color: var(--text); }
.pv-path {
  flex: 1;
  min-width: 0;
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 13px;
  overflow: hidden;
  white-space: nowrap;
}
.pv-dir { color: var(--text-dim); overflow: hidden; text-overflow: ellipsis; }
.pv-name { font-weight: 700; }

.pv-body { flex: 1; min-height: 0; overflow: hidden; }
.pv-center {
  height: 100%;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 10px;
  color: var(--text-dim);
  padding: 20px;
  text-align: center;
}
.pv-empty-title { font-size: 14px; font-weight: 600; color: var(--text); }
.pv-empty-hint { font-size: 12.5px; margin-bottom: 10px; }

.pv-text {
  margin: 0;
  padding: 18px 22px;
  font-family: var(--mono);
  font-size: 12.5px;
  line-height: 1.65;
  white-space: pre-wrap;
  word-break: break-word;
  user-select: text;
  color: var(--text);
  height: 100%;
  overflow: auto;
}
.pv-image {
  height: 100%;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 18px;
  overflow: hidden;
  background:
    conic-gradient(var(--bg-hover) 0 25%, transparent 0 50%, var(--bg-hover) 0 75%, transparent 0) 0 0 / 22px 22px;
}
.pv-image img {
  border-radius: 8px;
  box-shadow: var(--shadow-card);
  transition: transform 0.1s ease;
  transform-origin: center center;
  user-select: none;
  -webkit-user-drag: none;
}

.pv-foot {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 10px 14px;
  border-top: 1px solid var(--border);
}
.pv-size { font-size: 12.5px; color: var(--text-dim); }

/* Image zoom controls */
.zoom-controls {
  display: flex;
  align-items: center;
  gap: 4px;
  background: var(--bg-soft);
  border: 1px solid var(--border);
  border-radius: 8px;
  padding: 2px 4px;
}
.zoom-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 24px;
  height: 24px;
  border: none;
  background: transparent;
  color: var(--text-dim);
  border-radius: 5px;
  cursor: pointer;
}
.zoom-btn:hover:not(:disabled) { background: var(--bg-hover); color: var(--text); }
.zoom-btn:disabled { opacity: 0.35; cursor: default; }
.zoom-level {
  font-size: 11px;
  font-weight: 600;
  color: var(--text-dim);
  min-width: 36px;
  text-align: center;
  font-variant-numeric: tabular-nums;
}

.spinner {
  width: 28px;
  height: 28px;
  border-radius: 50%;
  border: 3px solid var(--border);
  border-top-color: var(--accent);
  animation: spin 0.8s linear infinite;
}
@keyframes spin { to { transform: rotate(360deg); } }
</style>
