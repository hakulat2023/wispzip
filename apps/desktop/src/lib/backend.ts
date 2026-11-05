// WispZip frontend transport layer: desktop mode goes through Tauri IPC; in a plain browser
// (opening `pnpm dev` directly) it switches to the built-in mock data, with zero component changes.

import type { UnlistenFn } from "@tauri-apps/api/event";
import { createMockInvoke } from "./backend.mock";

export const isTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

export interface EntryInfo {
  name: string;
  isDir: boolean;
  size: number;
  compressedSize?: number;
  mtime?: number;
  mode?: number;
}

export type Phase = "prepare" | "compress" | "extract" | "finish";

export interface ProgressEvent {
  jobId: number;
  phase: Phase;
  currentFile?: string;
  processedBytes: number;
  totalBytes?: number;
  outputBytes: number;
  filesDone: number;
  filesTotal?: number;
}

export interface JobDoneEvent {
  jobId: number;
  ok: boolean;
  error?: { kind: string; message: string };
  result?: Record<string, unknown>;
}

export interface Detected {
  format: string;
  classification: "archive" | "single_stream";
  supportsPassword: boolean;
  levelRange: { min: number; max: number };
}

export interface PreviewResult {
  /** text | image | binary */
  kind: "text" | "image" | "binary";
  mime?: string;
  /** Text content, or a data URL for images */
  content: string;
  truncated: boolean;
  size: number;
}

// ---- Event subscription (Tauri events / mock bus, dual implementation) --------

type Handler = (payload: unknown) => void;
const mockBus = new Map<string, Set<Handler>>();

async function subscribe<T>(event: string, cb: (payload: T) => void): Promise<UnlistenFn> {
  if (isTauri) {
    const { listen } = await import("@tauri-apps/api/event");
    return listen<T>(event, (ev) => cb(ev.payload));
  }
  let set = mockBus.get(event);
  if (!set) mockBus.set(event, (set = new Set()));
  const h = cb as Handler;
  set.add(h);
  return () => set!.delete(h);
}

function mockEmit<T>(event: string, payload: T) {
  mockBus.get(event)?.forEach((h) => h(payload));
}

export function onProgress(cb: (e: ProgressEvent) => void): Promise<UnlistenFn> {
  return subscribe<ProgressEvent>("wispzip:progress", cb);
}

export function onJobDone(cb: (e: JobDoneEvent) => void): Promise<UnlistenFn> {
  return subscribe<JobDoneEvent>("wispzip:done", cb);
}

// ---- Commands ---------------------------------------------------------------

async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<T>(cmd, args);
  }
  return mockInvoke<T>(cmd, args ?? {});
}

export const pickFiles = (filters?: string[]) =>
  invoke<string[] | null>("pick_files", { filters });

export const pickFolder = () => invoke<string | null>("pick_folder");

export const pickSave = (fileName: string, filters?: string[]) =>
  invoke<string | null>("pick_save", { fileName, filters });

export const detectArchive = (path: string) => invoke<Detected>("detect_archive", { path });

export const statPath = (path: string) =>
  invoke<{ size: number; isDir: boolean }>("stat_path", { path });

export const listArchive = (path: string, encoding = "auto", password?: string) =>
  invoke<EntryInfo[]>("list_archive", { path, encoding, password });

export interface ScanStats {
  files: number;
  dirs: number;
  bytes: number;
  skipped: number;
}

export const scanInputs = (inputs: string[], excludeHidden = false) =>
  invoke<ScanStats>("scan_inputs", { inputs, excludeHidden });

export const startCompress = (args: {
  inputs: string[];
  out: string;
  level?: number;
  storeOnly?: boolean;
  password?: string;
  splitBytes?: number;
  excludeHidden?: boolean;
}) => invoke<number>("start_compress", args);

export const startExtract = (args: {
  archive: string;
  dest: string;
  entries?: string[];
  conflict?: string;
  encoding?: string;
  password?: string;
  stripPrefix?: string;
}) => invoke<number>("start_extract", args);

export const cancelJob = (jobId: number) => invoke<boolean>("cancel_job", { jobId });

export const pauseJob = (jobId: number) => invoke<boolean>("pause_job", { jobId });

export const resumeJob = (jobId: number) => invoke<boolean>("resume_job", { jobId });

export const launchArgs = () => invoke<string[]>("launch_args");

/** Claim file paths cached by the Rust side before the webview was ready (macOS "Open With"). */
export const takeOpenedPaths = () => invoke<string[]>("take_opened_paths");

export function onLaunchArgs(cb: (args: string[]) => void): Promise<UnlistenFn> {
  return subscribe<string[]>("wispzip:launch-args", cb);
}

export function onOpenFile(cb: (paths: string[]) => void): Promise<UnlistenFn> {
  return subscribe<string[]>("wispzip:open-file", cb);
}

/** Open-settings request triggered by the macOS "Settings..." menu item and similar actions. */
export function onOpenSettings(cb: () => void): Promise<UnlistenFn> {
  return subscribe<null>("wispzip:open-settings", () => cb());
}

export const revealPath = (path: string) => invoke<void>("reveal_path", { path });

export const openPath = (path: string) => invoke<void>("open_path", { path });

export const installFinderServices = () => invoke<void>("install_finder_services");

/** Extract a single entry to a temp dir and open it with the default app. Returns the path written. */
export const openEntry = (args: {
  archive: string;
  entry: string;
  encoding?: string;
  password?: string;
}) => invoke<string>("open_entry", args);

/** Read entry content for the built-in preview (text truncated to 256KB, images become data URLs). */
export const previewEntry = (args: {
  archive: string;
  entry: string;
  encoding?: string;
  password?: string;
}) => invoke<PreviewResult>("preview_entry", args);

// ---- utils ----------------------------------------------------------------

export function fmtBytes(n: number): string {
  if (!n || n <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let i = 0;
  let v = n;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${i === 0 ? v.toFixed(0) : v.toFixed(1)} ${units[i]}`;
}

export function fileName(p: string): string {
  return p.split(/[\\/]/).pop() ?? p;
}

// ---- mock (non-Tauri environments only) --------------------------------------

const mockInvoke = createMockInvoke(mockEmit);
