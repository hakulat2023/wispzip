// File handling composable: file picking, drag-and-drop, launch-argument handling.

import { ref } from "vue";
import {
  pickFiles,
  pickFolder,
  type ScanStats,
} from "../lib/backend";
import { fileName } from "../lib/backend";
import { useArchive } from "./useArchive";

const ARCHIVE_EXTS = ["zip", "7z", "rar", "tar", "gz", "tgz", "bz2", "tbz2", "xz", "txz", "zst", "tzst", "zstd"];

// archive is injected by the caller: an archive opened via drag/drop or launch args must share App's state.
export function useFileHandling(archive: ReturnType<typeof useArchive>) {
  const { openArchive, parentOf } = archive;

  // Compress confirmation state
  const compressInputs = ref<string[]>([]);
  const compressScan = ref<ScanStats | null>(null);
  const compressScanning = ref(false);
  const compressDefaultOut = ref("");

  function looksLikeArchive(path: string): boolean {
    const n = fileName(path).toLowerCase();
    return ARCHIVE_EXTS.some((e) => n.endsWith("." + e)) || /\.\d{3}$/.test(n);
  }

  async function beginCompress(paths: string[]) {
    if (!paths.length) return;
    compressInputs.value = paths;
    compressScan.value = null;
    compressScanning.value = true;
    const defFormat = localStorage.getItem("wispzip.def.format") ?? "zip";
    const first = paths[0];
    const base = fileName(first).replace(/\.[^.\\/]+$/, "") || fileName(first);
    compressDefaultOut.value = `${parentOf(first)}${base}.${defFormat === "tar.zst" ? "tar.zst" : defFormat}`;
    // scanInputs is called outside because it needs the exclude-hidden-files setting
    return { paths, defFormat };
  }

  async function handleDropped(paths: string[]): Promise<{ action: "open"; path: string } | { action: "compress"; paths: string[] } | null> {
    if (!paths.length) return null;
    if (paths.length === 1 && looksLikeArchive(paths[0])) {
      await openArchive(paths[0]);
      return { action: "open", path: paths[0] };
    }
    await beginCompress(paths);
    return { action: "compress", paths };
  }

  async function chooseFiles(): Promise<string[] | null> {
    const picked = await pickFiles();
    return picked;
  }

  async function chooseFolder(): Promise<string | null> {
    const dir = await pickFolder();
    return dir;
  }

  async function chooseCompress(): Promise<string[] | null> {
    const picked = await pickFiles();
    return picked;
  }

  async function chooseExtract(): Promise<string | null> {
    const picked = await pickFiles(["zip", "7z", "rar", "tar", "gz", "bz2", "xz", "zst"]);
    if (picked?.length) {
      await openArchive(picked[0]);
      return picked[0];
    }
    return null;
  }

  /** `wispzip-app c <path>` / `x <path>` / `xhere <path>` -- Windows context-menu entry points. */
  async function handleLaunch(args: string[]): Promise<{
    action: "compress" | "open" | "xhere";
    path?: string;
    paths?: string[];
  } | null> {
    if (args.length >= 2 && args[0] === "c") {
      await beginCompress([args[1]]);
      return { action: "compress", paths: [args[1]] };
    }
    if (args.length >= 2 && args[0] === "xhere") {
      await openArchive(args[1]);
      return { action: "xhere", path: args[1] };
    }
    if (args.length >= 2 && args[0] === "x") {
      await openArchive(args[1]);
      return { action: "open", path: args[1] };
    }
    if (args.length >= 1 && looksLikeArchive(args[0])) {
      await openArchive(args[0]);
      return { action: "open", path: args[0] };
    }
    return null;
  }

  return {
    compressInputs,
    compressScan,
    compressScanning,
    compressDefaultOut,
    looksLikeArchive,
    beginCompress,
    handleDropped,
    chooseFiles,
    chooseFolder,
    chooseCompress,
    chooseExtract,
    handleLaunch,
  };
}
