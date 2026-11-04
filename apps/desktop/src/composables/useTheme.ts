import { onMounted, onUnmounted, ref, watch } from "vue";

/** Theme preference: follow system / force light / force dark. */
export type ThemeMode = "system" | "light" | "dark";

const STORAGE_KEY = "wispzip.theme";

function readStored(): ThemeMode {
  try {
    const v = localStorage.getItem(STORAGE_KEY);
    if (v === "light" || v === "dark" || v === "system") return v;
  } catch { /* private mode or unavailable: fall back to the default */ }
  return "system";
}

function systemPrefersDark(): boolean {
  if (typeof window === "undefined") return false;
  return window.matchMedia("(prefers-color-scheme: dark)").matches;
}

/** Combine the user preference and the system preference into the theme actually applied. */
function resolveEffective(mode: ThemeMode): "light" | "dark" {
  if (mode === "light") return "light";
  if (mode === "dark") return "dark";
  return systemPrefersDark() ? "dark" : "light";
}

function applyAttr(effective: "light" | "dark") {
  if (typeof document === "undefined") return;
  document.documentElement.setAttribute("data-theme", effective);
}

/**
 * Singleton: called once at the App level.
 * - `mode` reflects the user's choice (persisted).
 * - `effective` is the resolved theme applied to the DOM.
 * - `cycle()` rotates system -> light -> dark -> system.
 */
export function useTheme() {
  const mode = ref<ThemeMode>(readStored());

  const effective = ref<"light" | "dark">(resolveEffective(mode.value));

  // Apply to <html> immediately to avoid a first-paint flash (with [data-theme] in style.css)
  applyAttr(effective.value);

  // Watch system theme changes -- only react while mode === 'system'
  let mq: MediaQueryList | null = null;
  const onMqChange = () => {
    if (mode.value === "system") {
      effective.value = resolveEffective("system");
    }
  };
  onMounted(() => {
    if (typeof window !== "undefined") {
      mq = window.matchMedia("(prefers-color-scheme: dark)");
      mq.addEventListener("change", onMqChange);
    }
  });
  onUnmounted(() => {
    if (mq) mq.removeEventListener("change", onMqChange);
  });

  // mode changed -> re-resolve + persist + apply
  watch(mode, (m) => {
    try { localStorage.setItem(STORAGE_KEY, m); } catch { /* ignore */ }
    effective.value = resolveEffective(m);
  });

  // effective changed -> write the DOM attribute
  watch(effective, applyAttr);

  function setMode(m: ThemeMode) {
    mode.value = m;
  }

  function cycle() {
    const next: ThemeMode =
      mode.value === "system" ? "light"
        : mode.value === "light" ? "dark"
          : "system";
    mode.value = next;
  }

  return { mode, effective, setMode, cycle };
}
