<script setup lang="ts">
// Generic floating menu: anchored positioning, close on outside click/Esc, separators, icon items, keyboard nav.
import { nextTick, onBeforeUnmount, onMounted, ref } from "vue";
import AppIcon from "./AppIcon.vue";

export interface MenuItem {
  label: string;
  icon?: string;
  danger?: boolean;
  sep?: boolean; // insert a separator before this item
  disabled?: boolean;
  action: () => void;
}

const props = defineProps<{
  x: number;
  y: number;
  items: MenuItem[];
}>();
const emit = defineEmits<{ close: [] }>();

const el = ref<HTMLElement | null>(null);
const pos = ref({ x: props.x, y: props.y });
const focusIndex = ref(-1);

// Collect the indices of interactive menu items
function getNavigableIndices(): number[] {
  return props.items.map((item, i) => i).filter((i) => !props.items[i].disabled);
}

function focusItem(index: number) {
  focusIndex.value = index;
  nextTick(() => {
    const btns = el.value?.querySelectorAll<HTMLButtonElement>(".cmenu-item");
    btns?.[index]?.focus();
  });
}

function onDocPointer(e: PointerEvent) {
  if (el.value && !el.value.contains(e.target as Node)) emit("close");
}

function onKey(e: KeyboardEvent) {
  const navIndices = getNavigableIndices();
  if (!navIndices.length) return;

  switch (e.key) {
    case "Escape":
      e.stopPropagation(); // close only the menu, not the dialog beneath it
      emit("close");
      break;
    case "ArrowDown": {
      e.preventDefault();
      const curPos = navIndices.indexOf(focusIndex.value);
      const nextPos = curPos < navIndices.length - 1 ? curPos + 1 : 0;
      focusItem(navIndices[nextPos]);
      break;
    }
    case "ArrowUp": {
      e.preventDefault();
      const curPos = navIndices.indexOf(focusIndex.value);
      const nextPos = curPos > 0 ? curPos - 1 : navIndices.length - 1;
      focusItem(navIndices[nextPos]);
      break;
    }
    case "Home": {
      e.preventDefault();
      focusItem(navIndices[0]);
      break;
    }
    case "End": {
      e.preventDefault();
      focusItem(navIndices[navIndices.length - 1]);
      break;
    }
    case "Enter":
    case " ": {
      if (focusIndex.value >= 0) {
        e.preventDefault();
        const item = props.items[focusIndex.value];
        if (item && !item.disabled) run(item);
      }
      break;
    }
    case "Tab": {
      // Tab also closes the menu
      e.preventDefault();
      emit("close");
      break;
    }
  }
}

function run(item: MenuItem) {
  if (item.disabled) return;
  emit("close");
  item.action();
}

onMounted(() => {
  document.addEventListener("pointerdown", onDocPointer, true);
  document.addEventListener("keydown", onKey, true);
  // Prevent overflow past the viewport edge
  nextTick(() => {
    if (!el.value) return;
    const r = el.value.getBoundingClientRect();
    let { x, y } = pos.value;
    if (x + r.width > window.innerWidth - 8) x = window.innerWidth - r.width - 8;
    if (y + r.height > window.innerHeight - 8) y = window.innerHeight - r.height - 8;
    pos.value = { x: Math.max(8, x), y: Math.max(8, y) };

    // Auto-focus the first item
    const navIndices = getNavigableIndices();
    if (navIndices.length) focusItem(navIndices[0]);
  });
});
onBeforeUnmount(() => {
  document.removeEventListener("pointerdown", onDocPointer, true);
  document.removeEventListener("keydown", onKey, true);
});
</script>

<template>
  <div ref="el" class="cmenu" role="menu" :style="{ left: `${pos.x}px`, top: `${pos.y}px` }">
    <template v-for="(item, i) in props.items" :key="i">
      <div v-if="item.sep" class="cmenu-sep" role="separator" />
      <button
        class="cmenu-item"
        :class="{ danger: item.danger, focused: focusIndex === i }"
        :disabled="item.disabled"
        role="menuitem"
        tabindex="-1"
        @click="run(item)"
        @mouseenter="focusIndex = i"
      >
        <AppIcon v-if="item.icon" :name="item.icon" :size="15" aria-hidden="true" />
        <span>{{ item.label }}</span>
      </button>
    </template>
  </div>
</template>

<style scoped>
.cmenu {
  position: fixed;
  z-index: 90;
  min-width: 168px;
  padding: 5px;
  background: var(--menu-bg);
  border: 1px solid var(--border);
  border-radius: 12px;
  box-shadow: var(--shadow-pop);
  backdrop-filter: blur(20px);
  animation: cmenu-in 0.1s ease;
}
@keyframes cmenu-in {
  from { opacity: 0; transform: scale(0.98); }
  to { opacity: 1; transform: scale(1); }
}
.cmenu-item {
  display: flex;
  align-items: center;
  gap: 9px;
  width: 100%;
  padding: 7px 10px;
  border: none;
  background: transparent;
  border-radius: 8px;
  font-size: 13px;
  color: var(--text);
  cursor: pointer;
  text-align: left;
  outline: none;
}
.cmenu-item:hover, .cmenu-item.focused { background: var(--accent); color: #fff; }
.cmenu-item:disabled { opacity: 0.4; cursor: default; }
.cmenu-item:disabled:hover { background: transparent; color: var(--text); }
.cmenu-item.danger { color: var(--red); }
.cmenu-item.danger:hover, .cmenu-item.danger.focused { background: var(--red); color: #fff; }
.cmenu-sep { height: 1px; margin: 5px 8px; background: var(--border); }
</style>
