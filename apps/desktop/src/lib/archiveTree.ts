// Build a read-only filesystem index from a flat EntryInfo[]: children by directory, subtree aggregates,
// and expansion of a selection into exact entry names. Separator is always "/" (names carry no trailing slash).

import type { EntryInfo } from "./backend";

export interface DirStat {
  files: number;
  dirs: number;
  size: number;
  compSize: number;
}

const EMPTY_STAT: DirStat = { files: 0, dirs: 0, size: 0, compSize: 0 };

export class ArchiveIndex {
  private byDir = new Map<string, EntryInfo[]>();
  private stats = new Map<string, DirStat>();
  /** Flat list of all entries (directories included), used for search and selection expansion */
  readonly all: EntryInfo[];

  constructor(entries: EntryInfo[]) {
    // Many archivers (e.g. GitHub Actions artifacts) omit explicit directory entries; directories exist only
    // implicitly via file paths. Missing directory nodes are synthesized so browsing matches explicit dirs.
    const ensured = new Set<string>();
    const ensureDir = (dir: string) => {
      if (!dir || ensured.has(dir)) return;
      ensured.add(dir);
      const i = dir.lastIndexOf("/");
      const parent = i < 0 ? "" : dir.slice(0, i);
      const name = i < 0 ? dir : dir.slice(i + 1);
      ensureDir(parent);
      let list = this.byDir.get(parent);
      if (!list) this.byDir.set(parent, (list = []));
      if (!list.some((e) => e.isDir && e.name === name)) {
        list.push({ name, isDir: true, size: 0 } as EntryInfo);
      }
    };

    for (const e of entries) {
      // Defensive normalization: Rust already strips trailing slashes; this is a fallback (a trailing slash would create an empty name)
      const name = e.name.replace(/\\/g, "/").replace(/\/+$/, "");
      const i = name.lastIndexOf("/");
      const dir = i < 0 ? "" : name.slice(0, i);
      const base = i < 0 ? name : name.slice(i + 1);
      ensureDir(dir);
      if (e.isDir) ensured.add(name);
      let list = this.byDir.get(dir);
      if (!list) this.byDir.set(dir, (list = []));
      // children entries are stored as relative basenames; full paths are joined by walk()/the caller
      list.push({ ...e, name: base });
    }
    this.aggregate("");
    const flat: EntryInfo[] = [];
    const walk = (dir: string) => {
      for (const e of this.children(dir)) {
        const full = dir ? `${dir}/${e.name}` : e.name;
        flat.push({ ...e, name: full }); // the flat "all" table uses full paths
        if (e.isDir) walk(full);
      }
    };
    walk("");
    this.all = flat;
  }

  children(dir: string): EntryInfo[] {
    return this.byDir.get(dir) ?? [];
  }

  hasDir(dir: string): boolean {
    return this.byDir.has(dir);
  }

  /** dir subtree aggregate: file count / dir count / raw size / compressed size */
  stat(dir: string): DirStat {
    return this.stats.get(dir) ?? EMPTY_STAT;
  }

  private aggregate(dir: string): DirStat {
    const acc: DirStat = { files: 0, dirs: 0, size: 0, compSize: 0 };
    for (const e of this.children(dir)) {
      if (e.isDir) {
        acc.dirs++;
        const sub = this.aggregate(dir ? `${dir}/${e.name}` : e.name);
        acc.files += sub.files;
        acc.dirs += sub.dirs;
        acc.size += sub.size;
        acc.compSize += sub.compSize;
      } else {
        acc.files++;
        acc.size += e.size || 0;
        acc.compSize += e.compressedSize || 0;
      }
    }
    this.stats.set(dir, acc);
    return acc;
  }

  /** Selected files/dirs -> exact file entry names (used as the entries filter for start_extract). */
  expandToFiles(selected: Iterable<string>): string[] {
    const out = new Set<string>();
    for (const name of selected) {
      const normalized = name.replace(/\\/g, "/");
      const i = normalized.lastIndexOf("/");
      const dir = i < 0 ? "" : normalized.slice(0, i);
      const base = i < 0 ? normalized : normalized.slice(i + 1);
      const entry = this.children(dir).find((e) => e.name === base);
      if (entry && !entry.isDir) {
        out.add(normalized);
      } else {
        const prefix = `${normalized}/`;
        for (const e of this.all) {
          if (!e.isDir && e.name.replace(/\\/g, "/").startsWith(prefix)) out.add(e.name);
        }
      }
    }
    return [...out];
  }
}
