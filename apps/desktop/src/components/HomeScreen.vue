<script setup lang="ts">
// Home: big title + drop zone (primary button expands to file/folder picking) + feature cards + formats.
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import AppIcon from "./AppIcon.vue";
import ContextMenu, { type MenuItem } from "./ContextMenu.vue";
import { t } from "../lib/i18n";
import { isTauri } from "../lib/backend";

const emit = defineEmits<{
  choose: []; // pick files (archives -> browse, anything else -> compress)
  chooseFolder: []; // pick a folder to compress
  compress: [];
  extract: [];
}>();

const FORMATS = ["ZIP", "7Z", "RAR", "TAR", "GZ", "XZ", "ZST"];

/* Primary button dropdown: explicitly separates "pick files" from "pick folder" */
const menuOpen = ref(false);
const menuPos = ref({ x: 0, y: 0 });
const menuItems = computed<MenuItem[]>(() => [
  { label: t("home.chooseFiles"), icon: "file", action: () => emit("choose") },
  { label: t("home.chooseFolder"), icon: "folder", action: () => emit("chooseFolder") },
]);
function openMenu(e: MouseEvent) {
  const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
  menuPos.value = { x: r.right - 190, y: r.bottom + 8 };
  menuOpen.value = true;
}

/* Drag feedback: driven by Tauri drag events */
const dragging = ref(false);
let unlistenDrag: (() => void) | null = null;

onMounted(async () => {
  if (!isTauri) return;
  try {
    const { getCurrentWebview } = await import("@tauri-apps/api/webview");
    unlistenDrag = await getCurrentWebview().onDragDropEvent((ev) => {
      if (ev.payload.type === "over") dragging.value = true;
      else if (ev.payload.type === "drop" || ev.payload.type === "cancel") dragging.value = false;
    });
  } catch {
    // Ignore outside a Tauri environment
  }
});

onBeforeUnmount(() => {
  unlistenDrag?.();
});
</script>

<template>
  <main class="home">
    <section class="drop" :class="{ dragging }" role="button" :aria-label="t('home.dropTitle')" @click="emit('choose')">
      <div class="drop-blob">
        <AppIcon name="package" :size="44" aria-hidden="true" />
        <span class="spark s1" /><span class="spark s2" /><span class="spark s3" /><span class="spark s4" />
      </div>
      <div class="drop-title">{{ t("home.dropTitle") }}</div>
      <div class="drop-hint">{{ t("home.dropHint") }}</div>
      <button class="primary choose-btn" aria-label="选择文件" @click.stop="emit('choose')">
        <AppIcon name="folder-open" :size="18" aria-hidden="true" />
        <span>{{ t("home.chooseBtn") }}</span>
        <span class="caret" :title="t('home.moreWays')" @click.stop="openMenu">
          <AppIcon name="chevron-down" :size="15" aria-hidden="true" />
        </span>
      </button>
    </section>

    <div class="or"><span>{{ t("home.or") }}</span></div>

    <div class="quick">
      <button class="quick-btn" :aria-label="t('home.compress')" @click="emit('compress')">
        <span class="quick-ico tone-archive"><AppIcon name="package" :size="20" aria-hidden="true" /></span>
        <span class="quick-label">{{ t("home.compress") }}<small>{{ t("home.compressHint") }}</small></span>
        <AppIcon class="quick-go" name="chevron-right" :size="16" aria-hidden="true" />
      </button>
      <button class="quick-btn" :aria-label="t('home.openArchive')" @click="emit('extract')">
        <span class="quick-ico tone-folder"><AppIcon name="folder-open" :size="20" aria-hidden="true" /></span>
        <span class="quick-label">{{ t("home.openArchive") }}<small>{{ t("home.openHint") }}</small></span>
        <AppIcon class="quick-go" name="chevron-right" :size="16" aria-hidden="true" />
      </button>
    </div>

    <div class="formats">
      <div class="fmt-label">{{ t("home.formats") }}</div>
      <div class="fmt-row">
        <div class="fmt-safe"><AppIcon name="shield" :size="15" aria-hidden="true" /> {{ t("home.safety") }}</div>
        <div class="fmt-chips" role="list" aria-label="支持的格式">
          <span v-for="f in FORMATS" :key="f" role="listitem">{{ f }}</span>
        </div>
        <div aria-hidden="true" />
      </div>
    </div>

    <ContextMenu v-if="menuOpen" :x="menuPos.x" :y="menuPos.y" :items="menuItems" @close="menuOpen = false" />
  </main>
</template>

<style scoped>
.home {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: 12px;
  padding: 4px 36px 16px;
  max-width: 720px;
  margin: 0 auto;
  width: 100%;
  overflow-y: auto;
}

.drop {
  flex: 1;
  min-height: 230px;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 5px;
  padding: 28px 20px;
  border: 1.5px dashed var(--border-strong);
  border-radius: 22px;
  background: color-mix(in srgb, var(--accent) 4%, transparent);
  cursor: pointer;
  transition: border-color 0.15s, background 0.15s, transform 0.12s;
  text-align: center;
}
.drop:hover { border-color: var(--accent); background: var(--accent-soft); }
.drop.dragging {
  border-color: var(--accent);
  background: var(--accent-soft);
  transform: scale(1.01);
  box-shadow: 0 0 0 4px var(--accent-glow);
}
.drop.dragging .drop-blob {
  transform: scale(1.08);
  box-shadow: 0 16px 40px var(--accent-glow);
}
.drop.dragging .drop-title {
  color: var(--accent);
}
.drop-blob {
  position: relative;
  width: 84px;
  height: 84px;
  border-radius: 28px;
  display: flex;
  align-items: center;
  justify-content: center;
  color: #fff;
  background: linear-gradient(135deg, var(--accent), var(--accent-2));
  box-shadow: 0 12px 30px var(--accent-glow);
  margin-bottom: 12px;
  transition: transform 0.2s ease, box-shadow 0.2s ease;
}
/* Star specks around the gradient tile */
.spark { position: absolute; background: var(--accent); border-radius: 2px; transform: rotate(45deg); }
.s1 { width: 9px; height: 9px; top: -14px; right: -28px; opacity: 0.45; }
.s2 { width: 6px; height: 6px; top: -30px; right: 4px; opacity: 0.26; }
.s3 { width: 7px; height: 7px; bottom: -10px; left: -32px; opacity: 0.32; }
.s4 { width: 5px; height: 5px; top: 10px; left: -26px; opacity: 0.24; }
.drop-title { font-size: 19px; font-weight: 800; transition: color 0.15s; }
.drop-hint { font-size: 12.5px; color: var(--text-dim); }
.choose-btn {
  margin-top: 16px;
  display: inline-flex;
  align-items: center;
  gap: 9px;
  padding: 10px 11px 10px 22px;
  border-radius: 13px;
  font-size: 14px;
}
.choose-btn .caret {
  margin-left: 16px;
  width: 26px;
  height: 26px;
  border-radius: 8px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  background: rgba(255, 255, 255, 0.18);
}
.choose-btn .caret:hover { background: rgba(255, 255, 255, 0.32); }

.or {
  display: flex;
  align-items: center;
  gap: 14px;
  color: var(--text-dim);
  font-size: 11.5px;
}
.or::before, .or::after { content: ""; flex: 1; height: 1px; background: var(--border); }

.quick { display: flex; gap: 14px; }
.quick-btn {
  flex: 1;
  display: flex;
  align-items: center;
  gap: 13px;
  padding: 15px 16px;
  border: 1px solid var(--border);
  border-radius: 16px;
  background: var(--panel);
  box-shadow: var(--shadow-card);
  cursor: pointer;
  text-align: left;
  transition: border-color 0.15s, transform 0.1s;
}
.quick-btn:hover { border-color: var(--accent); }
.quick-btn:active { transform: scale(0.98); }
.quick-ico {
  width: 42px;
  height: 42px;
  border-radius: 13px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  flex: none;
  background: color-mix(in srgb, var(--chip) 15%, transparent);
  color: var(--chip);
}
.tone-archive { --chip: var(--t-archive); }
.tone-folder { --chip: var(--t-folder); }
.quick-label { display: flex; flex-direction: column; gap: 1px; font-size: 13.5px; font-weight: 700; }
.quick-label small { font-size: 11.5px; font-weight: 400; color: var(--text-dim); }
.quick-go { margin-left: auto; color: var(--text-dim); }

.formats { margin-top: 2px; }
.fmt-label { text-align: center; font-size: 11.5px; color: var(--text-dim); margin-bottom: 8px; }
.fmt-row {
  display: grid;
  grid-template-columns: 1fr auto 1fr;
  align-items: center;
  gap: 12px;
}
.fmt-safe {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 12px;
  color: var(--text-dim);
  white-space: nowrap;
}
.fmt-safe :deep(svg) { color: var(--green); }
.fmt-chips { display: flex; gap: 10px; flex-wrap: wrap; justify-content: center; }
.fmt-chips span {
  padding: 6px 15px;
  border-radius: 10px;
  background: var(--bg-hover);
  font-size: 11.5px;
  font-weight: 600;
  letter-spacing: 0.06em;
  color: var(--text-dim);
}
@media (max-width: 640px) {
  .home { padding-left: 20px; padding-right: 20px; }
  .fmt-row { grid-template-columns: 1fr; }
  .fmt-safe { display: none; }
}
</style>
