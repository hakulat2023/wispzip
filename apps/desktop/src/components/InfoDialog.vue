<script setup lang="ts">
// File info: raw size + compressed size + ratio -- the unique value of an archive browser over a file manager.
import { computed } from "vue";
import AppIcon from "./AppIcon.vue";
import FileIcon from "./FileIcon.vue";
import { fmtBytes } from "../lib/backend";
import { fmtDateTime } from "../lib/datetime";
import { kindOf } from "../lib/filekind";
import { t } from "../lib/i18n";
import { onEsc } from "../lib/esc";
import type { DirStat } from "../lib/archiveTree";

const props = defineProps<{
  archiveName: string;
  /** entry mode: an entry inside the archive (name is the full path) */
  entry?: { name: string; isDir: boolean; size: number; compressedSize?: number; mtime?: number } | null;
  entryStat?: DirStat | null;
  /** archive mode: whole-archive stats */
  archiveStat?: DirStat | null;
  archiveOnDisk?: number;
}>();

const emit = defineEmits<{ extract: []; close: [] }>();

const isDir = computed(() => !!props.entry?.isDir);
const displayName = computed(() => {
  if (!props.entry) return props.archiveName;
  return props.entry.name.split("/").pop() ?? props.entry.name;
});
const location = computed(() => {
  if (!props.entry) return props.archiveName;
  const i = props.entry.name.lastIndexOf("/");
  const dirPart = i < 0 ? "" : props.entry.name.slice(0, i);
  return dirPart ? `${props.archiveName} / ${dirPart}` : `${props.archiveName} /`;
});
const typeLabel = computed(() => {
  if (!props.entry) return t("info.kindArchive");
  if (props.entry.isDir) return t("info.kindFolder");
  const k = kindOf(props.entry.name, false);
  if (k.kind === "file" && k.label === t("info.kindFile")) return t("info.kindFile");
  return t("info.kindSuffixed", { label: k.label });
});

const origSize = computed(() => {
  if (!props.entry) return props.archiveOnDisk ?? 0;
  return props.entry.isDir ? props.entryStat?.size ?? 0 : props.entry.size;
});
const compSize = computed(() => {
  if (!props.entry) return props.archiveStat?.compSize ?? 0;
  return props.entry.isDir ? props.entryStat?.compSize ?? 0 : props.entry.compressedSize ?? 0;
});
const ratio = computed(() => {
  if (!origSize.value || compSize.value <= 0) return null;
  return Math.round((1 - compSize.value / origSize.value) * 100);
});
const saved = computed(() => (ratio.value && ratio.value > 0 ? fmtBytes(Math.max(0, origSize.value - compSize.value)) : null));

// Close the dialog on Esc
onEsc(() => emit("close"));
</script>

<template>
  <div class="mask" @click.self="emit('close')">
    <div class="dialog">
      <header class="dlg-head">
        <div class="dlg-title">{{ entry ? t("info.fileTitle") : t("info.archiveTitle") }}</div>
        <button class="dlg-close" @click="emit('close')"><AppIcon name="close" :size="15" /></button>
      </header>

      <div class="who">
        <FileIcon :name="displayName" :is-dir="isDir" :size="44" />
        <div class="who-name">{{ displayName }}</div>
      </div>

      <div class="stats">
        <div class="stat">
          <div class="stat-v">{{ fmtBytes(origSize) }}</div>
          <div class="stat-k">{{ entry ? t("info.origSize") : t("info.contentSize") }}</div>
        </div>
        <div class="stat">
          <div class="stat-v">{{ fmtBytes(compSize) }}</div>
          <div class="stat-k">{{ t("info.compSize") }}</div>
        </div>
        <div class="stat accent">
          <div class="stat-v">{{ ratio != null && ratio > 0 ? `${ratio}%` : "—" }}</div>
          <div class="stat-k">{{ t("info.ratio") }}</div>
        </div>
      </div>
      <div v-if="saved" class="saved">{{ t("info.saved", { size: saved }) }}</div>

      <div class="rows">
        <div class="r"><span>{{ t("browser.colType") }}</span><span>{{ typeLabel }}</span></div>
        <div v-if="entry?.isDir && entryStat" class="r"><span>{{ t("info.contains") }}</span><span>{{ t("browser.filesDirs", { n: entryStat.files, d: entryStat.dirs }) }}</span></div>
        <div v-if="!entry && archiveStat" class="r"><span>{{ t("info.contains") }}</span><span>{{ t("browser.filesDirs", { n: archiveStat.files, d: archiveStat.dirs }) }}</span></div>
        <div v-if="entry?.mtime" class="r"><span>{{ t("browser.colMtime") }}</span><span>{{ fmtDateTime(entry.mtime) }}</span></div>
        <div class="r"><span>{{ t("info.location") }}</span><span class="mono">{{ location }}</span></div>
        <div class="r"><span>{{ t("info.inArchive") }}</span><span>{{ archiveName }}</span></div>
      </div>

      <footer class="dlg-actions">
        <button class="ghost" @click="emit('close')">{{ t("common.close") }}</button>
        <button v-if="entry" class="primary" @click="emit('extract')">{{ t("common.extract") }}</button>
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
  width: min(380px, 92vw);
  background: var(--panel);
  border: 1px solid var(--border);
  border-radius: 16px;
  box-shadow: var(--shadow-pop);
  padding: 18px 20px 20px;
  display: flex;
  flex-direction: column;
  gap: 14px;
  animation: pop 0.16s cubic-bezier(0.2, 0.9, 0.3, 1.2);
}
@keyframes pop { from { opacity: 0; transform: scale(0.96) translateY(6px); } }

.dlg-head { display: flex; align-items: center; justify-content: space-between; }
.dlg-title { font-size: 14px; font-weight: 700; color: var(--text-dim); }
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

.who { display: flex; flex-direction: column; align-items: center; gap: 10px; }
.who-name {
  font-size: 15px;
  font-weight: 700;
  max-width: 100%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.stats {
  display: grid;
  grid-template-columns: 1fr 1fr 1fr;
  gap: 8px;
}
.stat {
  background: var(--bg-soft);
  border: 1px solid var(--border);
  border-radius: 12px;
  padding: 10px 6px;
  text-align: center;
}
.stat-v { font-size: 15px; font-weight: 700; font-variant-numeric: tabular-nums; }
.stat.accent .stat-v { color: var(--accent); }
.stat-k { font-size: 11px; color: var(--text-dim); margin-top: 2px; }
.saved { text-align: center; font-size: 12px; color: var(--green); font-weight: 600; margin-top: -6px; }

.rows { display: flex; flex-direction: column; gap: 2px; }
.r {
  display: flex;
  justify-content: space-between;
  gap: 16px;
  font-size: 12.5px;
  padding: 5px 2px;
}
.r > span:first-child { color: var(--text-dim); flex: none; }
.r > span:last-child { text-align: right; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }

.dlg-actions { display: flex; gap: 10px; justify-content: flex-end; }
</style>
