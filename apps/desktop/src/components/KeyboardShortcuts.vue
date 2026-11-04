<script setup lang="ts">
// Shortcut help panel: lists every available shortcut.
import AppIcon from "./AppIcon.vue";
import { t } from "../lib/i18n";
import { onEsc } from "../lib/esc";

const emit = defineEmits<{ close: [] }>();

const isMac = /Mac/i.test(navigator.userAgent);
const mod = isMac ? "⌘" : "Ctrl";

interface ShortcutGroup {
  title: string;
  shortcuts: { keys: string[]; description: string }[];
}

const groups: ShortcutGroup[] = [
  {
    title: t("shortcuts.navigation"),
    shortcuts: [
      { keys: ["Backspace"], description: t("shortcuts.navUp") },
      { keys: ["Enter"], description: t("shortcuts.openSelected") },
      { keys: ["Esc"], description: t("shortcuts.clearOrClose") },
    ],
  },
  {
    title: t("shortcuts.selection"),
    shortcuts: [
      { keys: [mod, "A"], description: t("shortcuts.selectAll") },
      { keys: ["Shift", "Click"], description: t("shortcuts.rangeSelect") },
      { keys: [mod, "Click"], description: t("shortcuts.multiSelect") },
    ],
  },
  {
    title: t("shortcuts.search"),
    shortcuts: [
      { keys: [mod, "F"], description: t("shortcuts.focusSearch") },
      { keys: ["Esc"], description: t("shortcuts.clearSearch") },
    ],
  },
];

onEsc(() => emit("close"));
</script>

<template>
  <div class="mask" role="dialog" :aria-label="t('shortcuts.title')" @click.self="emit('close')">
    <div class="help">
      <header class="help-head">
        <div class="help-title">
          <AppIcon name="keyboard" :size="18" aria-hidden="true" />
          {{ t("shortcuts.title") }}
        </div>
        <button class="help-close" :aria-label="t('common.close')" @click="emit('close')">
          <AppIcon name="close" :size="15" />
        </button>
      </header>

      <div class="help-body">
        <div v-for="group in groups" :key="group.title" class="shortcut-group">
          <div class="group-title">{{ group.title }}</div>
          <div v-for="sc in group.shortcuts" :key="sc.description" class="shortcut-row">
            <span class="shortcut-desc">{{ sc.description }}</span>
            <span class="shortcut-keys">
              <kbd v-for="(key, i) in sc.keys" :key="i">{{ key }}</kbd>
            </span>
          </div>
        </div>
      </div>

      <footer class="help-foot">
        <span class="help-hint">{{ t("shortcuts.hint", { mod }) }}</span>
        <button class="ghost" @click="emit('close')">{{ t("common.close") }}</button>
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

.help {
  width: min(420px, 92vw);
  background: var(--panel);
  border: 1px solid var(--border);
  border-radius: 16px;
  box-shadow: var(--shadow-pop);
  padding: 20px;
  display: flex;
  flex-direction: column;
  gap: 16px;
  animation: pop 0.16s cubic-bezier(0.2, 0.9, 0.3, 1.2);
}
@keyframes pop { from { opacity: 0; transform: scale(0.96) translateY(6px); } }

.help-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
}
.help-title {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 15px;
  font-weight: 700;
}
.help-close {
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
.help-close:hover { background: var(--bg-hover); color: var(--text); }

.help-body {
  display: flex;
  flex-direction: column;
  gap: 16px;
}
.shortcut-group {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.group-title {
  font-size: 11.5px;
  font-weight: 700;
  color: var(--text-dim);
  text-transform: uppercase;
  letter-spacing: 0.05em;
}
.shortcut-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 4px 0;
  font-size: 13px;
}
.shortcut-desc { color: var(--text); }
.shortcut-keys {
  display: flex;
  align-items: center;
  gap: 4px;
}
.shortcut-keys kbd {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-width: 24px;
  height: 22px;
  padding: 0 6px;
  border: 1px solid var(--border);
  border-radius: 5px;
  background: var(--bg-soft);
  font-family: inherit;
  font-size: 11px;
  font-weight: 600;
  color: var(--text-dim);
  box-shadow: 0 1px 0 var(--border);
}

.help-foot {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding-top: 4px;
  border-top: 1px solid var(--border);
}
.help-hint { font-size: 12px; color: var(--text-dim); }
</style>
