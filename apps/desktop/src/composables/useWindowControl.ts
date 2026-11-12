// Window control composable: custom minimize/maximize/close buttons for Windows.

import { ref } from "vue";
import { isTauri } from "../lib/backend";

const isWin = /\bWindows\b/i.test(navigator.userAgent);

/** Returned by Tauri's event listeners; call it to detach. */
type UnlistenFn = () => void;

export function useWindowControl() {
  const winMaximized = ref(false);

  // Generic in the callback's result so a listener registration can hand its
  // unlisten function back to the caller instead of being dropped here.
  async function withWin<T>(
    cb: (w: import("@tauri-apps/api/window").Window) => Promise<T>,
  ): Promise<T | undefined> {
    if (!isTauri) return undefined;
    const { getCurrentWindow } = await import("@tauri-apps/api/window");
    return cb(getCurrentWindow());
  }

  const winMinimize = () => withWin((w) => w.minimize());
  const winToggleMax = () => withWin((w) => w.toggleMaximize());
  const winClose = () => withWin((w) => w.close());

  /**
   * Mirror the live maximized state. Returns the unlisten function so the
   * caller can detach it on unmount; `null` when the custom title bar is not
   * used (macOS / browser mock).
   */
  async function syncMaximizedState(): Promise<UnlistenFn | null> {
    if (!isTauri || !isWin) return null;
    const unlisten = await withWin(async (w) => {
      winMaximized.value = await w.isMaximized();
      return w.onResized(async () => {
        winMaximized.value = await w.isMaximized();
      });
    });
    return unlisten ?? null;
  }

  return {
    isWin,
    winMaximized,
    winMinimize,
    winToggleMax,
    winClose,
    syncMaximizedState,
  };
}
