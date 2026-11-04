<script setup lang="ts">
// App orchestration: home / archive browser / compress confirm / progress / done, plus the overlays.
// Main flow: right-click or drop an archive -> browser; view, open and extract without unpacking.
import { computed, onMounted, ref } from "vue";
import AppIcon from "./components/AppIcon.vue";
import HomeScreen from "./components/HomeScreen.vue";
import BrowserScreen from "./components/BrowserScreen.vue";
import CompressSheet from "./components/CompressSheet.vue";
import ProgressView from "./components/ProgressView.vue";
import DoneView from "./components/DoneView.vue";
import ExtractDialog from "./components/ExtractDialog.vue";
import PreviewOverlay from "./components/PreviewOverlay.vue";
import InfoDialog from "./components/InfoDialog.vue";
import SettingsModal from "./components/SettingsModal.vue";
import KeyboardShortcuts from "./components/KeyboardShortcuts.vue";
import { t } from "./lib/i18n";
import { onEsc } from "./lib/esc";
import {
  isTauri,
  scanInputs,
  startCompress,
  startExtract,
  revealPath,
  openEntry,
  statPath,
  launchArgs,
  onProgress,
  onJobDone,
  onLaunchArgs,
  onOpenFile,
  onOpenSettings,
  fmtBytes,
  fileName,
  type EntryInfo,
  type JobDoneEvent,
} from "./lib/backend";
import { useToast } from "./composables/useToast";
import { useArchive } from "./composables/useArchive";
import { useJobProgress, type Screen } from "./composables/useJobProgress";
import { useFileHandling } from "./composables/useFileHandling";
import { useSettings } from "./composables/useSettings";
import { useWindowControl } from "./composables/useWindowControl";
import { useTheme } from "./composables/useTheme";

/* Title-bar platform differences: macOS uses the native traffic lights (Overlay, floating top-left);
   Windows drops the system frame so the topbar draws minimize/maximize/close on the right and
   acts as the drag region. Other platforms keep the system frame. */
const isMac = /Mac/i.test(navigator.userAgent);

/* ================= Composables ================= */

const { toastMsg, toastBusy, toast, toastErr } = useToast();
const archive = useArchive();
const {
  archivePath, entries, listLoading, needsPassword, password,
  index, archiveName, parentOf, openArchive, resetArchive,
} = archive;
const {
  mode, jobId, progress, paused,
  startJob, updateProgress, togglePause, cancelCurrentJob, clearJob,
} = useJobProgress();
const {
  compressInputs, compressScan, compressScanning, compressDefaultOut,
  handleDropped, chooseFiles, chooseFolder, chooseCompress, chooseExtract, handleLaunch,
} = useFileHandling(archive);
const { defFormat, defPreset, defExcl, saveSettings, getExcludeHidden, getFormat } = useSettings();
const { isWin, winMaximized, winMinimize, winToggleMax, winClose, syncMaximizedState } = useWindowControl();
const { mode: themeMode, cycle: cycleTheme } = useTheme();

const themeIcon = computed(() => {
  if (themeMode.value === "light") return "sun";
  if (themeMode.value === "dark") return "moon";
  return "monitor";
});
const themeTooltip = computed(() => {
  if (themeMode.value === "light") return t("theme.light");
  if (themeMode.value === "dark") return t("theme.dark");
  return t("theme.system");
});

/* ================= Global state ================= */

type ScreenType = Screen;
const screen = ref<ScreenType>("home");
const booted = ref(false);
const aboutOpen = ref(false);
const appVersion = ref("");

// Done view
const doneTitle = ref("");
const doneSubject = ref("");
const doneDetail = ref<string | null>(null);
const donePath = ref("");

// Extract dialog
const extractOpen = ref(false);
const extractSelection = ref<string[] | null>(null);
const extractHere = ref(false);

// Preview / info / settings
const previewPath = ref<string | null>(null);
const infoTarget = ref<EntryInfo | null>(null);
const infoArchive = ref(false);
const infoOnDisk = ref(0);
const settingsOpen = ref(false);
const shortcutsOpen = ref(false);

/* ================= Compress flow ================= */

async function beginCompressFlow(paths: string[]) {
  const result = await handleDropped(paths);
  if (result?.action === "compress") {
    screen.value = "compress";
    const excludeHidden = getExcludeHidden();
    scanInputs(paths, excludeHidden)
      .then((s) => { compressScan.value = s; compressScanning.value = false; })
      .catch(() => { compressScanning.value = false; });
  }
}

async function onCompressStart(args: { out: string; level: number; password?: string; splitBytes?: number; excludeHidden: boolean }) {
  try {
    const newJobId = await startCompress({
      inputs: compressInputs.value,
      out: args.out,
      level: args.level,
      password: args.password,
      splitBytes: args.splitBytes,
      excludeHidden: args.excludeHidden,
    });
    startJob("compress", newJobId, "compress");
    screen.value = "progress";
  } catch (e) {
    toastErr(e);
  }
}

/* ================= Browser actions ================= */

async function openWithSystem(entry: EntryInfo) {
  toast(t("toast.opening", { name: fileName(entry.name) }), true);
  try {
    await openEntry({
      archive: archivePath.value,
      entry: entry.name,
      password: password.value || undefined,
    });
    toastMsg.value = "";
  } catch (e) {
    toastErr(e);
  }
}

// Extract stats (file count / size shown in the dialog header)
const entryByName = computed(() => {
  const m = new Map<string, EntryInfo>();
  for (const e of entries.value) m.set(e.name, e);
  return m;
});
const extractStats = computed(() => {
  if (!extractSelection.value) {
    const s = index.value.stat("");
    return { files: s.files, bytes: s.size };
  }
  let files = 0;
  let bytes = 0;
  for (const n of extractSelection.value) {
    const e = entryByName.value.get(n);
    if (e && !e.isDir) { files++; bytes += e.size || 0; }
    else { const s = index.value.stat(n); files += s.files; bytes += s.size; }
  }
  return { files, bytes };
});

/**
 * Longest common ancestor directory of the selected file paths. Used to strip the internal directory
 * structure when extracting a selection. Example: ["Mango/a.txt", "Mango/deep/b.txt"] -> "Mango"
 * while ["a.txt", "Mango/b.txt"] -> "" (mixed at root, nothing stripped).
 */
function commonAncestorDir(paths: string[]): string | undefined {
  if (!paths.length) return undefined;
  const dirs = paths.map((p) => {
    const n = p.replace(/\\/g, "/");
    const i = n.lastIndexOf("/");
    return i < 0 ? "" : n.slice(0, i);
  });
  let ancestor = dirs[0];
  for (let i = 1; i < dirs.length; i++) {
    while (ancestor && dirs[i] !== ancestor) {
      const j = ancestor.lastIndexOf("/");
      ancestor = j < 0 ? "" : ancestor.slice(0, j);
    }
    if (!ancestor) return undefined;
  }
  return ancestor || undefined;
}

function onBrowserExtract(selection: string[], quick: boolean) {
  const destBase = parentOf(archivePath.value).replace(/[\\/]+$/, "");
  if (quick) {
    const expanded = selection.length ? index.value.expandToFiles(selection) : undefined;
    const strip = expanded ? commonAncestorDir(expanded) : undefined;
    void runExtract({
      dest: destBase,
      entries: expanded,
      conflict: "rename",
      encoding: "auto",
      password: password.value || undefined,
      stripPrefix: strip,
    });
    return;
  }
  extractSelection.value = selection.length ? index.value.expandToFiles(selection) : null;
  extractOpen.value = true;
}

async function onExtractStart(args: { dest: string; entries: string[] | undefined; conflict: string; encoding: string; password?: string }) {
  extractOpen.value = false;
  const strip = extractSelection.value ? commonAncestorDir(extractSelection.value) : undefined;
  await runExtract({
    dest: args.dest,
    entries: args.entries,
    conflict: args.conflict,
    encoding: args.encoding,
    password: args.password || password.value || undefined,
    stripPrefix: strip,
  });
}

async function runExtract(args: { dest: string; entries?: string[]; conflict: string; encoding: string; password?: string; stripPrefix?: string }) {
  try {
    const newJobId = await startExtract({
      archive: archivePath.value,
      dest: args.dest,
      entries: args.entries,
      conflict: args.conflict,
      encoding: args.encoding,
      password: args.password,
      stripPrefix: args.stripPrefix,
    });
    startJob("extract", newJobId, "browser");
    screen.value = "progress";
  } catch (e) {
    toastErr(e);
  }
}

async function onInfo(target: EntryInfo | null) {
  infoTarget.value = target;
  infoArchive.value = target === null;
  if (target === null) {
    try {
      infoOnDisk.value = (await statPath(archivePath.value)).size;
    } catch { infoOnDisk.value = 0; }
  }
}

const infoExtractSelection = computed(() => {
  if (!infoTarget.value) return [];
  return [infoTarget.value.name];
});

function infoExtract() {
  const name = infoTarget.value?.name;
  infoTarget.value = null;
  if (name) onBrowserExtract([name], false);
}

/* ================= Progress / done ================= */

async function cancelJobUI() {
  const returnTo = await cancelCurrentJob();
  screen.value = returnTo;
}

function goHome() {
  screen.value = "home";
  clearJob();
  resetArchive();
  previewPath.value = null;
  extractOpen.value = false;
}

/* ================= Entry points: drop / context menu / file association ============ */

async function handleDroppedPaths(paths: string[]) {
  const result = await handleDropped(paths);
  if (result?.action === "compress") {
    screen.value = "compress";
    const excludeHidden = getExcludeHidden();
    scanInputs(paths, excludeHidden)
      .then((s) => { compressScan.value = s; compressScanning.value = false; })
      .catch(() => { compressScanning.value = false; });
  } else if (result?.action === "open") {
    screen.value = "browser";
  }
}

async function chooseFilesFlow() {
  const picked = await chooseFiles();
  if (picked?.length) await handleDroppedPaths(picked);
}
async function chooseCompressFlow() {
  const picked = await chooseCompress();
  if (picked?.length) await beginCompressFlow(picked);
}
async function chooseFolderFlow() {
  const dir = await chooseFolder();
  if (dir) await beginCompressFlow([dir]);
}
async function chooseExtractFlow() {
  const path = await chooseExtract();
  if (path) screen.value = "browser";
}

async function handleLaunchFlow(args: string[]) {
  const result = await handleLaunch(args);
  if (result?.action === "compress") {
    screen.value = "compress";
    const excludeHidden = getExcludeHidden();
    scanInputs(result.paths!, excludeHidden)
      .then((s) => { compressScan.value = s; compressScanning.value = false; })
      .catch(() => { compressScanning.value = false; });
  } else if (result?.action === "open") {
    screen.value = "browser";
  } else if (result?.action === "xhere") {
    screen.value = "browser";
    if (!needsPassword.value) {
      extractSelection.value = null;
      extractHere.value = true;
      extractOpen.value = true;
    }
  }
}

/* ================= wiring ================= */

const unlisteners: (() => void)[] = [];
onMounted(async () => {
  try {
    unlisteners.push(await onProgress((e) => updateProgress(e)));
    unlisteners.push(await onJobDone((e: JobDoneEvent) => {
      if (e.jobId !== jobId.value) return;
      clearJob();
      if (e.ok && e.result) {
        if (mode.value === "compress") {
          const r = e.result as { outputBytes: number; inputBytes: number; outputPath: string };
          doneTitle.value = t("done.compressTitle");
          doneSubject.value = fileName(r.outputPath);
          const p = r.inputBytes > 0 ? Math.round((1 - r.outputBytes / r.inputBytes) * 100) : 0;
          doneDetail.value = r.inputBytes > 0
            ? t("done.savedDetail", {
                input: fmtBytes(r.inputBytes),
                output: fmtBytes(r.outputBytes),
                p: Math.max(0, p),
              })
            : null;
          donePath.value = r.outputPath;
        } else {
          const r = e.result as { files: number; skipped: number; dest: string };
          doneTitle.value = t("done.extractTitle");
          doneSubject.value = fileName(r.dest);
          doneDetail.value = r.skipped
            ? t("done.extractSkipped", { n: r.files, s: r.skipped })
            : t("done.extractDetail", { n: r.files });
          donePath.value = r.dest;
        }
        screen.value = "done";
      } else {
        toastErr(e.error);
        screen.value = "home";
      }
    }));
    if (isTauri) {
      const { getCurrentWebview } = await import("@tauri-apps/api/webview");
      unlisteners.push(await getCurrentWebview().onDragDropEvent((ev) => {
        if (ev.payload.type === "drop") void handleDroppedPaths(ev.payload.paths);
      }));
    }
    unlisteners.push(await onOpenFile((paths) => {
      if (paths[0]) void openArchive(paths[0]).then(() => { screen.value = "browser"; });
    }));
    unlisteners.push(await onLaunchArgs((args) => void handleLaunchFlow(args)));
    unlisteners.push(await onOpenSettings(() => { settingsOpen.value = true; }));
    void handleLaunchFlow(await launchArgs());
    // On a cold start the Opened event may precede listener registration; claim the cached one
    const pending = await invoke<string[]>("take_opened_paths");
    if (pending.length) void handleLaunchFlow(pending);
    if (isTauri) {
      if (isWin) {
        const unlisten = await syncMaximizedState();
        if (unlisten) unlisteners.push(unlisten);
      }
      try {
        const { getVersion } = await import("@tauri-apps/api/app");
        appVersion.value = await getVersion();
      } catch { appVersion.value = ""; }
    }
  } catch (e) {
    console.error("boot", e);
  } finally {
    booted.value = true;
  }
});

// About dialog: close on Esc
onEsc(() => {
  if (aboutOpen.value) aboutOpen.value = false;
});

// Cmd+/ or Ctrl+/ opens the shortcut help
function onGlobalKeydown(e: KeyboardEvent) {
  if ((isMac ? e.metaKey : e.ctrlKey) && e.key === "/") {
    e.preventDefault();
    shortcutsOpen.value = !shortcutsOpen.value;
  }
}
if (typeof window !== "undefined") {
  window.addEventListener("keydown", onGlobalKeydown);
}

function closeSettings() {
  settingsOpen.value = false;
}

function onSaveSettings(v: { defFormat: string; defPreset: string; defExcludeHidden: boolean }) {
  saveSettings(v);
  settingsOpen.value = false;
}
</script>

<template>
  <div v-if="booted" class="shell" :class="{ mac: isMac }">
    <header class="topbar" data-tauri-drag-region>
      <div class="spacer" data-tauri-drag-region />
      <button class="tbtn" :title="themeTooltip" @click="cycleTheme">
        <AppIcon :name="themeIcon" :size="22" />
      </button>
      <button class="tbtn" :title="t('topbar.about')" @click="aboutOpen = true">
        <AppIcon name="help" :size="36" />
      </button>
      <button class="tbtn" :title="t('common.settings')" @click="settingsOpen = true">
        <AppIcon name="settings" :size="36" />
      </button>
      <div v-if="isWin" class="winctl">
        <button class="wbtn" title="最小化" @click="winMinimize">
          <AppIcon name="win-min" :size="15" />
        </button>
        <button class="wbtn" :title="winMaximized ? '向下还原' : '最大化'" @click="winToggleMax">
          <AppIcon :name="winMaximized ? 'win-restore' : 'win-max'" :size="13" />
        </button>
        <button class="wbtn wclose" title="关闭" @click="winClose">
          <AppIcon name="close" :size="15" />
        </button>
      </div>
    </header>

    <HomeScreen
      v-if="screen === 'home'"
      @choose="chooseFilesFlow"
      @choose-folder="chooseFolderFlow"
      @compress="chooseCompressFlow"
      @extract="chooseExtractFlow"
    />

    <BrowserScreen
      v-else-if="screen === 'browser'"
      :archive-path="archivePath"
      :entries="entries"
      :loading="listLoading"
      :needs-password="needsPassword"
      @unlock="(pw) => openArchive(archivePath, pw)"
      @open="openWithSystem"
      @preview="(e) => (previewPath = e.name)"
      @extract="onBrowserExtract"
      @info="onInfo"
      @reveal-archive="revealPath(archivePath)"
      @reload="openArchive(archivePath, password || undefined)"
      @go-home="goHome"
    />

    <CompressSheet
      v-else-if="screen === 'compress'"
      :inputs="compressInputs"
      :scan="compressScan"
      :scanning="compressScanning"
      :default-out="compressDefaultOut"
      :default-format="defFormat"
      :default-preset="defPreset"
      :default-exclude-hidden="defExcl"
      @start="onCompressStart"
      @close="goHome"
    />

    <ProgressView
      v-else-if="screen === 'progress'"
      :title="mode === 'compress' ? t('progress.compressTitle') : t('progress.extractTitle')"
      :subject="mode === 'compress' ? fileName(compressDefaultOut) : archiveName"
      :progress="progress"
      :paused="paused"
      @toggle-pause="togglePause"
      @cancel="cancelJobUI"
    />

    <DoneView
      v-else-if="screen === 'done'"
      :title="doneTitle"
      :subject="doneSubject"
      :detail="doneDetail"
      @reveal="revealPath(donePath)"
      @continue="goHome"
      @done="mode === 'extract' && archivePath ? openArchive(archivePath, password || undefined).then(() => { screen = 'browser'; }) : goHome()"
    />

    <ExtractDialog
      v-if="extractOpen"
      :archive-path="archivePath"
      :archive-name="archiveName"
      :selection="extractSelection"
      :stats="extractStats"
      :default-dest="parentOf(archivePath)"
      :default-create-folder="!extractHere && !extractSelection"
      :password="password"
      @start="onExtractStart"
      @close="extractOpen = false; extractHere = false"
    />

    <PreviewOverlay
      v-if="previewPath"
      :archive-path="archivePath"
      :entry-path="previewPath"
      :password="password"
      @extract="(p) => { previewPath = null; onBrowserExtract([p], false); }"
      @close="previewPath = null"
    />

    <InfoDialog
      v-if="infoTarget || infoArchive"
      :archive-name="archiveName"
      :entry="infoTarget"
      :entry-stat="infoTarget ? index.stat(infoTarget.name) : null"
      :archive-stat="index.stat('')"
      :archive-on-disk="infoOnDisk"
      @extract="infoExtract"
      @close="infoTarget = null; infoArchive = false"
    />

    <SettingsModal
      v-if="settingsOpen"
      :def-format="defFormat"
      :def-preset="defPreset"
      :def-exclude-hidden="defExcl"
      @save="onSaveSettings"
      @close="closeSettings"
    />

    <KeyboardShortcuts
      v-if="shortcutsOpen"
      @close="shortcutsOpen = false"
    />

    <div v-if="aboutOpen" class="mask" @click.self="aboutOpen = false">
      <div class="about">
        <div class="about-mark"><AppIcon name="package" :size="30" /></div>
        <div v-if="appVersion" class="about-name"><span class="about-ver">v{{ appVersion }}</span></div>
        <div class="about-tag"><AppIcon name="shield" :size="14" /> {{ t("about.tagline") }}</div>
        <div class="about-fmt">ZIP · 7Z · RAR · TAR · GZ · XZ · ZST</div>
        <footer class="about-actions">
          <button class="ghost" @click="aboutOpen = false">{{ t("common.close") }}</button>
        </footer>
      </div>
    </div>

    <transition name="toast">
      <div v-if="toastMsg" class="toast" :class="{ busy: toastBusy }">
        <span v-if="toastBusy" class="toast-spin" />
        {{ toastMsg }}
      </div>
    </transition>
  </div>
</template>

<style scoped>
.shell {
  display: flex;
  flex-direction: column;
  height: 100%;
  user-select: none;
}
.topbar {
  display: flex;
  align-items: center;
  gap: 2px;
  height: 42px;
  padding: 0 10px;
  flex: none;
}
.mac .topbar { padding-left: 84px; }
.tbtn {
  border: none;
  background: transparent;
  color: var(--text-dim);
  width: 40px;
  height: 40px;
  border-radius: 10px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
}
.tbtn:hover { background: var(--bg-hover); color: var(--text); }
.spacer { flex: 1; }

/* Windows custom window controls (decorations: false) */
.winctl { display: flex; align-items: center; margin-left: 8px; }
.wbtn {
  border: none;
  background: transparent;
  color: var(--text-dim);
  width: 42px;
  height: 30px;
  border-radius: 8px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
}
.wbtn:hover { background: var(--bg-hover); color: var(--text); }
.wclose:hover { background: var(--red); color: #fff; }

/* About */
.mask {
  position: fixed;
  inset: 0;
  z-index: 60;
  background: var(--mask);
  backdrop-filter: blur(4px);
  display: flex;
  align-items: center;
  justify-content: center;
}
.about {
  width: min(320px, 92vw);
  background: var(--panel);
  border: 1px solid var(--border);
  border-radius: 16px;
  box-shadow: var(--shadow-pop);
  padding: 24px 20px 18px;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 8px;
  text-align: center;
}
.about-mark {
  width: 64px;
  height: 64px;
  border-radius: 20px;
  display: flex;
  align-items: center;
  justify-content: center;
  color: #fff;
  background: linear-gradient(135deg, var(--accent), var(--accent-2));
  box-shadow: 0 8px 22px var(--accent-glow);
  margin-bottom: 4px;
}
.about-name { font-size: 15px; font-weight: 700; }
.about-ver { font-size: 13px; font-weight: 600; color: var(--text-dim); }
.about-tag {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 12.5px;
  color: var(--text-dim);
}
.about-tag svg { color: var(--green); }
.about-fmt { font-size: 11px; color: var(--text-dim); letter-spacing: 0.06em; opacity: 0.85; }
.about-actions { margin-top: 10px; }

.toast {
  position: fixed;
  left: 50%;
  bottom: 26px;
  transform: translateX(-50%);
  z-index: 100;
  display: flex;
  align-items: center;
  gap: 8px;
  background: var(--menu-bg);
  border: 1px solid var(--border);
  color: var(--text);
  font-size: 12.5px;
  border-radius: 12px;
  padding: 9px 16px;
  box-shadow: var(--shadow-pop);
  max-width: 78vw;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.toast-spin {
  width: 12px;
  height: 12px;
  border-radius: 50%;
  border: 2px solid var(--border);
  border-top-color: var(--accent);
  animation: toast-spin 0.8s linear infinite;
  flex: none;
}
@keyframes toast-spin { to { transform: rotate(360deg); } }
.toast-enter-active, .toast-leave-active { transition: opacity 0.16s, transform 0.16s; }
.toast-enter-from, .toast-leave-to { opacity: 0; transform: translateX(-50%) translateY(8px); }
</style>
