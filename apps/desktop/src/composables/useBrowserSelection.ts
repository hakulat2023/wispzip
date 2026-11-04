// Browser selection composable: manages selection state and selection logic.
import { computed, ref } from "vue";
import type { EntryInfo } from "../lib/backend";
import { ArchiveIndex } from "../lib/archiveTree";

const isMac = /Mac/i.test(navigator.userAgent);

export interface Row {
  entry: EntryInfo;
  path: string;
  name: string;
  parentDir: string;
  stat: { files: number; size: number };
  parent?: boolean;
}

export function useBrowserSelection() {
  const selected = ref<Set<string>>(new Set());
  const activeIndex = ref<number>(-1);
  let anchor: string | null = null;

  function clear() {
    selected.value = new Set();
    anchor = null;
    activeIndex.value = -1;
  }

  function rowClick(e: MouseEvent, row: Row, i: number, rows: Row[], up: () => void) {
    if (row.parent) {
      up();
      return;
    }
    activeIndex.value = i;
    const meta = isMac ? e.metaKey : e.ctrlKey;
    if (e.shiftKey && anchor) {
      const a = rows.findIndex((r) => r.path === anchor);
      if (a >= 0) {
        const [lo, hi] = a < i ? [a, i] : [i, a];
        const next = new Set(meta ? selected.value : new Set<string>());
        for (let j = lo; j <= hi; j++) next.add(rows[j].path);
        selected.value = next;
        return;
      }
    }
    if (meta) {
      const next = new Set(selected.value);
      if (next.has(row.path)) next.delete(row.path);
      else next.add(row.path);
      selected.value = next;
      anchor = row.path;
      return;
    }
    selected.value = new Set([row.path]);
    anchor = row.path;
  }

  function selectAll(rows: Row[]) {
    selected.value = new Set(rows.filter((r) => !r.parent).map((r) => r.path));
  }

  function openRow(row: Row, cd: (path: string) => void, emitOpen: (entry: EntryInfo) => void) {
    if (row.parent) {
      return;
    }
    if (row.entry.isDir) cd(row.path);
    else emitOpen({ ...row.entry, name: row.path });
  }

  function calcSelectedStats(idx: ArchiveIndex, rows: Row[]) {
    let files = 0;
    let size = 0;
    for (const p of selected.value) {
      const s = idx.stat(p);
      const row = rows.find((r) => r.path === p);
      if (row && !row.entry.isDir) {
        files += 1;
        size += row.entry.size || 0;
      } else {
        files += s.files;
        size += s.size;
      }
    }
    return { files, size };
  }

  return {
    selected,
    activeIndex,
    clear,
    rowClick,
    selectAll,
    openRow,
    calcSelectedStats,
  };
}
