<script setup lang="ts">
// File type icon: soft color block + line icon, hue keyed by type (modern file-list style).
import { computed } from "vue";
import AppIcon from "./AppIcon.vue";
import { kindOf } from "../lib/filekind";

const props = withDefaults(defineProps<{ name: string; isDir: boolean; size?: number }>(), {
  size: 28,
});

const info = computed(() => kindOf(props.name, props.isDir));
</script>

<template>
  <span
    class="ficon"
    :class="info.tone"
    :style="{ width: `${props.size}px`, height: `${props.size}px` }"
  >
    <AppIcon :name="info.icon" :size="Math.round(props.size * 0.58)" />
  </span>
</template>

<style scoped>
.ficon {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border-radius: 8px;
  flex: none;
  background: color-mix(in srgb, var(--chip) 16%, transparent);
  color: var(--chip);
}
.tone-folder { --chip: var(--t-folder); }
.tone-archive { --chip: var(--t-archive); }
.tone-image { --chip: var(--t-image); }
.tone-video { --chip: var(--t-video); }
.tone-audio { --chip: var(--t-audio); }
.tone-pdf { --chip: var(--t-pdf); }
.tone-code { --chip: var(--t-code); }
.tone-text { --chip: var(--t-text); }
.tone-file { --chip: var(--t-file); }
</style>
