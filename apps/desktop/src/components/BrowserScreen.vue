<script setup lang="ts">
// Archive browser: browse an unextracted archive like Finder.
// Navigation history / breadcrumbs / sortable table / search / multi-select / context menu live here;
// open, preview, extract and info actions are handed to App via events.
import { computed, nextTick, ref, watch } from "vue";
import AppIcon from "./AppIcon.vue";
import FileIcon from "./FileIcon.vue";
import ContextMenu, { type MenuItem } from "./ContextMenu.vue";
import BrowserPasswordGate from "./BrowserPasswordGate.vue";
import BrowserStatusBar from "./BrowserStatusBar.vue";
import { ArchiveIndex } from "../lib/archiveTree";
import { kindOf } from "../lib/filekind";
import { fmtBytes } from "../lib/backend";
import { fmtDate } from "../lib/datetime";
import { t } from "../lib/i18n";
import type { EntryInfo } from "../lib/backend";
import { useBrowserNavigation } from "../composables/useBrowserNavigation";
import { useBrowserSearch } from "../composables/useBrowserSearch";
import { useBrowserSort } from "../composables/useBrowserSort";
import { useBrowserSelection, type Row } from "../composables/useBrowserSelection";

const props = defineProps<{
  archivePath: string;
  entries: EntryInfo[];
  loading: boolean;
  needsPassword: boolean;
}>();

const emit = defineEmits<{
  unlock: [password: string];
  open: [entry: EntryInfo];
  preview: [entry: EntryInfo];
  extract: [selection: string[], quick: boolean];
  info: [entry: EntryInfo | null];
  revealArchive: [];
  reload: [];
  goHome: [];
}>();

const idx = computed(() => new ArchiveIndex(props.entries));
const archiveName = computed(() => props.archivePath.split(/[\\/]/).pop() ?? props.archivePath);

// Composables
const { dir, canBack, canForward, canUp, cd, back, forward, up, reset: resetNav } = useBrowserNavigation();
const { search, debouncedSearch, searching, clear: clearSearch } = useBrowserSearch();
const { sortKey, sortAsc, setSort } = useBrowserSort();
const {
  selected, activeIndex, clear: clearSelection,
  rowClick, selectAll, openRow: openRowBase, calcSelectedStats,
} = useBrowserSelection();

const searchEl = ref<HTMLInputElement | null>(null);
const collator = new Intl.Collator(undefined, { numeric: true, sensitivity: "base" });

// Clear selection and search when navigation changes
watch(dir, () => {
  clearSelection();
  clearSearch();
  activeIndex.value = -1;
});

// Reset navigation when the archive path changes
watch(() => props.archivePath, () => {
  resetNav();
  clearSelection();
  clearSearch();
  activeIndex.value = -1;
});

// Build row data
const rows = computed<Row[]>(() => {
  const mk = (e: EntryInfo, parentDir: string, path: string): Row => {
    const i = path.lastIndexOf("/");
    const base = i < 0 ? path : path.slice(i + 1);
    return {
      entry: e,
      path,
      name: base,
      parentDir,
      stat: e.isDir ? idx.value.stat(path) : { files: 0, size: e.size || 0 },
    };
  };

  const parentRow: Row | null = !searching.value && dir.value
    ? {
        entry: { name: "..", isDir: true, size: 0 },
        path: "..",
        name: "..",
        parentDir: "",
        stat: { files: 0, size: 0 },
        parent: true,
      }
    : null;

  let list: Row[];
  if (searching.value) {
    const q = debouncedSearch.value.trim().toLowerCase();
    list = idx.value.all
      .filter((e) => e.name.toLowerCase().includes(q))
      .slice(0, 500)
      .map((e) => {
        const name = e.name.replace(/\\/g, "/");
        const i = name.lastIndexOf("/");
        return mk(e, i < 0 ? "" : name.slice(0, i), name);
      });
  } else {
    list = idx.value.children(dir.value).map((e) =>
      mk(e, dir.value, dir.value ? `${dir.value}/${e.name}` : e.name),
    );
  }

  if (parentRow) list = [parentRow, ...list];

  const dirFirst = (a: Row, b: Row) => Number(b.entry.isDir) - Number(a.entry.isDir);
  const k = sortKey.value;
  const sign = sortAsc.value ? 1 : -1;

  list.sort((a, b) => {
    if (searching.value) return collator.compare(a.path, b.path);
    const d = dirFirst(a, b);
    if (d) return d;
    switch (k) {
      case "size": return sign * (a.stat.size - b.stat.size) || collator.compare(a.name, b.name);
      case "type": return sign * collator.compare(kindOf(a.entry.name, a.entry.isDir).kind, kindOf(b.entry.name, b.entry.isDir).kind) || collator.compare(a.name, b.name);
      case "mtime": return sign * ((a.entry.mtime ?? 0) - (b.entry.mtime ?? 0)) || collator.compare(a.name, b.name);
      default: return sign * collator.compare(a.name, b.name);
    }
  });

  return list;
});

// Breadcrumbs
const crumbs = computed(() => {
  const parts = dir.value ? dir.value.split("/") : [];
  const segs: { name: string; path: string }[] = [{ name: archiveName.value, path: "" }];
  let acc = "";
  for (const p of parts) {
    acc = acc ? `${acc}/${p}` : p;
    segs.push({ name: p, path: acc });
  }
  return segs;
});

// Selection stats
const selStat = computed(() => calcSelectedStats(idx.value, rows.value));
const dirStat = computed(() => idx.value.stat(dir.value));

// Open row
function openRow(row: Row) {
  openRowBase(row, cd, (entry) => emit("open", entry));
}

// Row click
function handleRowClick(e: MouseEvent, row: Row, i: number) {
  rowClick(e, row, i, rows.value, up);
}

// Context menu
const menu = ref<{ x: number; y: number; items: MenuItem[] } | null>(null);

function isPreviewable(entry: EntryInfo): boolean {
  return kindOf(entry.name, entry.isDir).preview !== "none";
}

function contextItems(target: Row): MenuItem[] {
  const multi = selected.value.size > 1 && selected.value.has(target.path);
  if (!multi) {
    const acts: MenuItem[] = [
      { label: t("browser.ctxOpen"), icon: target.entry.isDir ? "folder-open" : "external", action: () => (target.entry.isDir ? cd(target.path) : emit("open", { ...target.entry, name: target.path })) },
    ];
    if (!target.entry.isDir && isPreviewable(target.entry)) {
      acts.push({ label: t("browser.ctxPreview"), icon: "eye", action: () => emit("preview", { ...target.entry, name: target.path }) });
    }
    acts.push(
      { label: t("browser.ctxExtractTo"), icon: "download", sep: true, action: () => emit("extract", [target.path], false) },
      { label: t("browser.ctxQuickExtract"), icon: "download", action: () => emit("extract", [target.path], true) },
      { label: t("browser.ctxFileInfo"), icon: "info", sep: true, action: () => emit("info", { ...target.entry, name: target.path }) },
    );
    return acts;
  }
  return [
    { label: t("browser.ctxExtractN", { n: selected.value.size }), icon: "download", action: () => emit("extract", [...selected.value], false) },
    { label: t("browser.ctxQuickExtract"), icon: "download", action: () => emit("extract", [...selected.value], true) },
    { label: t("browser.ctxFileInfo"), icon: "info", sep: true, disabled: true, action: () => {} },
  ];
}

function rowContext(e: MouseEvent, row: Row) {
  e.preventDefault();
  if (row.parent) {
    blankContext(e);
    return;
  }
  if (!selected.value.has(row.path)) {
    selected.value = new Set([row.path]);
  }
  menu.value = { x: e.clientX, y: e.clientY, items: contextItems(row) };
}

function blankContext(e: MouseEvent) {
  e.preventDefault();
  clearSelection();
  menu.value = {
    x: e.clientX,
    y: e.clientY,
    items: [
      { label: t("browser.ctxExtractAll"), icon: "download", action: () => emit("extract", [], false) },
      { label: t("browser.ctxReveal"), icon: "reveal", action: () => emit("revealArchive") },
      { label: t("browser.ctxReload"), icon: "refresh", sep: true, action: () => emit("reload") },
    ],
  };
}

const morePos = ref({ x: 0, y: 0 });
const moreItems = computed<MenuItem[]>(() => [
  { label: t("browser.ctxExtractAll"), icon: "download", action: () => emit("extract", [], false) },
  { label: t("browser.ctxReveal"), icon: "reveal", action: () => emit("revealArchive") },
  { label: t("browser.ctxReload"), icon: "refresh", sep: true, action: () => emit("reload") },
  { label: t("browser.ctxHome"), icon: "close", sep: true, action: () => emit("goHome") },
]);
const moreOpen = ref(false);

// Keyboard shortcuts
function onKeydown(e: KeyboardEvent) {
  if (props.needsPassword) return;
  const tag = (e.target as HTMLElement)?.tagName;
  if (tag === "INPUT" || tag === "TEXTAREA") {
    if (e.key === "Escape") (e.target as HTMLInputElement).blur();
    return;
  }

  const list = rows.value;
  if (e.key === "Escape") {
    if (searching.value) clearSearch();
    else clearSelection();
  } else if (e.key === "Backspace") {
    e.preventDefault();
    up();
  } else if (e.key === "Enter") {
    const first = list.find((r) => selected.value.has(r.path));
    if (first) openRow(first);
  } else if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "a") {
    e.preventDefault();
    selectAll(list);
  } else if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "f") {
    e.preventDefault();
    searchEl.value?.focus();
  } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
    e.preventDefault();
    if (list.length === 0) return;
    const delta = e.key === "ArrowDown" ? 1 : -1;
    const cur = activeIndex.value >= 0 ? activeIndex.value : (delta > 0 ? -1 : list.length);
    const next = Math.max(0, Math.min(list.length - 1, cur + delta));
    const row = list[next];
    if (!row || row.parent) return;

    activeIndex.value = next;

    if (e.shiftKey) {
      // Shift+arrow: extend from the anchor to the current focus (no anchor -> first item becomes anchor)
      let anchorPath = row.path;
      if (selected.value.size > 0) {
        let anchorIdx = -1;
        for (let j = 0; j < list.length; j++) {
          if (selected.value.has(list[j].path)) { anchorIdx = j; break; }
        }
        if (anchorIdx >= 0) anchorPath = list[anchorIdx].path;
      }
      const a = list.findIndex((r) => r.path === anchorPath);
      const [lo, hi] = a <= next ? [a, next] : [next, a];
      const range = new Set<string>();
      for (let j = lo; j <= hi; j++) {
        if (!list[j].parent) range.add(list[j].path);
      }
      selected.value = range;
    } else if (e.metaKey || e.ctrlKey) {
      // Mod+arrow: move focus and toggle the current item, keeping the anchor unchanged
      const ns = new Set(selected.value);
      if (ns.has(row.path)) ns.delete(row.path);
      else ns.add(row.path);
      selected.value = ns;
    } else {
      // Plain arrow: select the current item only
      selected.value = new Set([row.path]);
    }
  }
}

// Scroll the focused item into view after it changes
watch(activeIndex, (i) => {
  if (i < 0) return;
  nextTick(() => {
    const el = document.querySelector(`.tr[data-i="${i}"]`);
    el?.scrollIntoView({ block: "nearest", behavior: "smooth" });
  });
});

function handleExtractSelected() {
  emit("extract", [...selected.value], false);
}
</script>

<template>
  <div class="browser" tabindex="0" role="region" aria-label="压缩包浏览器" @keydown="onKeydown">
    <!-- Toolbar -->
    <div class="toolbar">
      <div class="nav">
        <button class="nav-btn" :disabled="!canBack" :title="t('browser.navBack')" aria-label="后退" @click="back">
          <AppIcon name="arrow-left" :size="16" />
        </button>
        <button class="nav-btn" :disabled="!canForward" :title="t('browser.navForward')" aria-label="前进" @click="forward">
          <AppIcon name="arrow-right" :size="16" />
        </button>
        <button class="nav-btn" :disabled="!canUp" :title="t('browser.navUp')" aria-label="上一级" @click="up">
          <AppIcon name="arrow-up" :size="16" />
        </button>
      </div>

      <nav class="crumbs" aria-label="路径导航">
        <template v-for="(c, i) in crumbs" :key="c.path">
          <AppIcon v-if="i > 0" name="chevron-right" :size="13" class="crumb-sep" aria-hidden="true" />
          <button class="crumb" :class="{ root: i === 0, on: i === crumbs.length - 1 }" :aria-current="i === crumbs.length - 1 ? 'location' : undefined" @click="cd(c.path)">
            <FileIcon v-if="i === 0" :name="archiveName" :is-dir="false" :size="18" aria-hidden="true" />
            <span class="crumb-name">{{ c.name }}</span>
          </button>
        </template>
        <span v-if="searching" class="crumb-search" aria-live="polite">{{ t("browser.searchingFor", { q: debouncedSearch.trim() }) }}</span>
      </nav>

      <div class="tools">
        <div class="searchbox" :class="{ active: searching }">
          <AppIcon name="search" :size="14" class="search-ico" aria-hidden="true" />
          <input ref="searchEl" v-model="search" :placeholder="t('common.search')" spellcheck="false" aria-label="搜索文件" />
          <button v-if="search" class="search-clear" aria-label="清除搜索" @click="search = ''">
            <AppIcon name="close" :size="12" />
          </button>
        </div>
        <button class="nav-btn" :title="t('common.more')" aria-label="更多操作" @click="(e) => { morePos = { x: e.clientX, y: e.clientY }; moreOpen = true; }">
          <AppIcon name="more" :size="16" />
        </button>
      </div>
    </div>

    <!-- Password gate -->
    <BrowserPasswordGate
      v-if="needsPassword"
      :archive-name="archiveName"
      @unlock="(pw) => emit('unlock', pw)"
      @cancel="emit('goHome')"
    />

    <!-- Table -->
    <template v-else>
      <div class="table" role="grid" aria-label="文件列表" @contextmenu="blankContext">
        <div class="thead" role="row">
          <button class="th th-name" role="columnheader" aria-sort="name" @click="setSort('name')">
            {{ t("browser.colName") }}
            <AppIcon v-if="sortKey === 'name' && !searching" :name="sortAsc ? 'chevron-up' : 'chevron-down'" :size="12" aria-hidden="true" />
          </button>
          <button class="th th-size" :class="{ right: true }" role="columnheader" aria-sort="size" @click="setSort('size')">
            {{ t("browser.colSize") }}
            <AppIcon v-if="sortKey === 'size' && !searching" :name="sortAsc ? 'chevron-up' : 'chevron-down'" :size="12" aria-hidden="true" />
          </button>
          <button class="th th-type" role="columnheader" aria-sort="type" @click="setSort('type')">
            {{ t("browser.colType") }}
            <AppIcon v-if="sortKey === 'type' && !searching" :name="sortAsc ? 'chevron-up' : 'chevron-down'" :size="12" aria-hidden="true" />
          </button>
          <button class="th th-mtime" role="columnheader" aria-sort="mtime" @click="setSort('mtime')">
            {{ t("browser.colMtime") }}
            <AppIcon v-if="sortKey === 'mtime' && !searching" :name="sortAsc ? 'chevron-up' : 'chevron-down'" :size="12" aria-hidden="true" />
          </button>
        </div>

        <div class="tbody" role="rowgroup">
          <div v-if="loading" class="state" role="status">
            <div class="spinner" aria-hidden="true" />
            <span>{{ t("browser.loading") }}</span>
          </div>

          <div v-else-if="rows.length === 0" class="state" role="status">
            <AppIcon :name="searching ? 'search' : 'folder-open'" :size="28" aria-hidden="true" />
            <span>{{ searching ? t("browser.noMatch") : t("browser.emptyDir") }}</span>
          </div>

          <template v-else>
          <div
            v-for="(r, i) in rows"
            :key="r.path"
            class="tr"
            role="row"
            :data-i="i"
            :class="{ sel: selected.has(r.path), dir: r.entry.isDir, active: activeIndex === i }"
            :aria-selected="selected.has(r.path)"
            @click="handleRowClick($event, r, i)"
            @dblclick="openRow(r)"
            @contextmenu.stop="rowContext($event, r)"
          >
            <span class="td td-name" role="gridcell">
              <FileIcon v-if="!r.parent" :name="r.name" :is-dir="r.entry.isDir" :size="26" aria-hidden="true" />
              <AppIcon v-else name="chevron-up" :size="22" aria-hidden="true" />
              <span class="fname">
                <span class="fname-text">{{ r.parent ? t("browser.parentRow") : r.name }}</span>
                <span v-if="searching && r.parentDir" class="fname-dir">{{ r.parentDir }}/</span>
              </span>
            </span>
            <span class="td td-size" role="gridcell">{{ r.parent ? "" : r.entry.isDir ? "—" : fmtBytes(r.entry.size) }}</span>
            <span class="td td-type" role="gridcell">{{ r.parent ? "" : kindOf(r.entry.name, r.entry.isDir).label }}</span>
            <span class="td td-mtime" role="gridcell">{{ r.parent ? "" : fmtDate(r.entry.mtime) }}</span>
          </div>
          </template>
        </div>
      </div>

      <!-- Status bar -->
      <BrowserStatusBar
        :selected-count="selected.size"
        :selected-size="selStat.size"
        :is-searching="searching"
        :search-results-count="rows.length"
        :dir-stat="dirStat"
        :current-dir="dir"
        @extract="handleExtractSelected"
      />
    </template>

    <ContextMenu v-if="moreOpen" :x="morePos.x" :y="morePos.y" :items="moreItems" @close="moreOpen = false" />
    <ContextMenu
      v-if="menu"
      :x="menu.x"
      :y="menu.y"
      :items="menu.items"
      @close="menu = null"
    />
  </div>
</template>

<style scoped>
.browser {
  flex: 1;
  display: flex;
  flex-direction: column;
  min-height: 0;
  outline: none;
}

/* ---- Toolbar ---- */
.toolbar {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px 14px 6px;
}
.nav { display: flex; gap: 2px; }
.nav-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 30px;
  height: 30px;
  padding: 0;
  border: none;
  background: transparent;
  border-radius: 8px;
  color: var(--text-dim);
  cursor: pointer;
}
.nav-btn:hover:not(:disabled) { background: var(--bg-hover); color: var(--text); }
.nav-btn:disabled { opacity: 0.35; cursor: default; }

.crumbs {
  display: flex;
  align-items: center;
  gap: 2px;
  min-width: 0;
  flex: 1;
  overflow: hidden;
}
.crumb {
  display: inline-flex;
  align-items: center;
  gap: 7px;
  padding: 5px 8px;
  border: none;
  background: transparent;
  border-radius: 8px;
  font-size: 13.5px;
  font-weight: 600;
  color: var(--text-dim);
  cursor: pointer;
  max-width: 220px;
}
.crumb:hover { background: var(--bg-hover); color: var(--text); }
.crumb.on { color: var(--text); }
.crumb.root { font-weight: 700; color: var(--text); }
.crumb-name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.crumb-sep { color: var(--border-strong); flex: none; }
.crumb-search { font-size: 13px; color: var(--accent); font-weight: 600; margin-left: 8px; }

.tools { display: flex; align-items: center; gap: 6px; }
.searchbox {
  display: flex;
  align-items: center;
  gap: 6px;
  width: 132px;
  height: 30px;
  padding: 0 8px;
  border-radius: 8px;
  background: var(--bg-soft);
  border: 1px solid var(--border);
  transition: width 0.18s ease, border-color 0.15s;
}
.searchbox:focus-within, .searchbox.active { width: 190px; border-color: var(--accent); }
.search-ico { color: var(--text-dim); flex: none; }
.searchbox input {
  border: none;
  background: transparent;
  padding: 0;
  height: 100%;
  font-size: 12.5px;
}
.search-clear {
  display: inline-flex;
  border: none;
  background: transparent;
  color: var(--text-dim);
  padding: 2px;
  cursor: pointer;
  border-radius: 4px;
}
.search-clear:hover { color: var(--text); }

/* ---- Table ---- */
.table {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  margin: 2px 10px 0;
  border: 1px solid var(--border);
  border-radius: 12px;
  background: var(--panel);
  overflow: hidden;
}
.thead {
  display: grid;
  grid-template-columns: minmax(0, 1fr) 92px 76px 112px;
  gap: 12px;
  padding: 0 12px;
  border-bottom: 1px solid var(--border);
  background: var(--bg-soft);
}
.th {
  display: inline-flex;
  align-items: center;
  gap: 3px;
  border: none;
  background: transparent;
  padding: 8px 0;
  font-size: 12px;
  font-weight: 600;
  color: var(--text-dim);
  cursor: pointer;
}
.th:hover { color: var(--text); }
.th.right { justify-content: flex-end; }
.th-mtime, .th-type { justify-content: flex-start; }
.tbody { flex: 1; overflow-y: auto; padding: 4px 0 8px; }
.tr {
  display: grid;
  grid-template-columns: minmax(0, 1fr) 92px 76px 112px;
  gap: 12px;
  align-items: center;
  height: 38px;
  margin: 0 6px;
  padding: 0 6px;
  border-radius: 8px;
  cursor: default;
}
.tr:hover { background: var(--bg-hover); }
.tr.sel { background: var(--accent-soft); }
.tr.sel:hover { background: var(--accent-soft); }
.tr.active { box-shadow: inset 2px 0 0 var(--accent); }
.tr.active:hover { background: var(--bg-hover); }
.tr.sel.active { box-shadow: inset 2px 0 0 var(--accent); }
.tr.sel.active:hover { background: var(--accent-soft); }
.td { font-size: 13px; color: var(--text); white-space: nowrap; }
.td-size { text-align: right; color: var(--text-dim); font-variant-numeric: tabular-nums; }
.td-type { color: var(--text-dim); }
.td-mtime { color: var(--text-dim); font-variant-numeric: tabular-nums; }
.td-name { display: flex; align-items: center; gap: 10px; min-width: 0; }
.fname { display: flex; flex-direction: column; min-width: 0; line-height: 1.2; }
.fname-text { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-weight: 500; }
.tr.dir .fname-text { font-weight: 600; }
.fname-dir { font-size: 11px; color: var(--text-dim); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }

.state {
  height: 100%;
  min-height: 180px;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 10px;
  color: var(--text-dim);
  font-size: 13px;
}

.spinner {
  width: 26px;
  height: 26px;
  border-radius: 50%;
  border: 3px solid var(--border);
  border-top-color: var(--accent);
  animation: spin 0.8s linear infinite;
}
@keyframes spin { to { transform: rotate(360deg); } }
</style>
