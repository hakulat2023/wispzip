<script setup lang="ts">
// Status bar component: shows stats for the current directory or selection.

import AppIcon from "./AppIcon.vue";
import { fmtBytes } from "../lib/backend";
import { t } from "../lib/i18n";

const props = defineProps<{
  selectedCount: number;
  selectedSize: number;
  isSearching: boolean;
  searchResultsCount: number;
  dirStat: { files: number; dirs: number; size: number };
  currentDir: string;
}>();

const emit = defineEmits<{
  extract: [];
}>();
</script>

<template>
  <footer class="statusbar">
    <template v-if="selectedCount > 0">
      <span>{{ t("browser.statusSelected", { n: selectedCount }) }} · {{ fmtBytes(selectedSize) }}</span>
      <span class="spacer" />
      <button class="status-act" aria-label="解压选中文件" @click="emit('extract')">{{ t("common.extract") }}</button>
    </template>
    <template v-else-if="isSearching">
      <span>{{ t("browser.statusResults", { n: searchResultsCount }) }}</span>
      <span class="spacer" />
      <span class="dim">{{ t("browser.searchAll") }}</span>
    </template>
    <template v-else>
      <span>{{ t("browser.nFiles", { n: dirStat.files }) }}<template v-if="dirStat.dirs"> · {{ t("browser.nFolders", { n: dirStat.dirs }) }}</template></span>
      <span class="spacer" />
      <span class="dim">{{ currentDir === "" ? t("browser.totalInArchive") : t("browser.total") }} {{ fmtBytes(dirStat.size) }}</span>
    </template>
  </footer>
</template>

<style scoped>
.statusbar {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 7px 20px 9px;
  font-size: 12px;
  color: var(--text-dim);
  border-top: 1px solid var(--border);
  background: var(--bg-soft);
}
.statusbar .dim { opacity: 0.85; }
.status-act {
  border: none;
  background: var(--accent-soft);
  color: var(--accent);
  font-size: 12px;
  font-weight: 600;
  border-radius: 7px;
  padding: 4px 12px;
  cursor: pointer;
}
.status-act:hover { background: var(--accent); color: #fff; }
</style>
