// Window control composable: custom minimize/maximize/close buttons for Windows.

import { ref } from "vue";
import { isTauri } from "../lib/backend";

const isWin = /\bWindows\b/i.test(navigator.userAgent);

export function useWindowControl() {
  const winMaximized = ref(false);

  async function withWin(cb: (w: import("@tauri-apps/api/window").Window) => Promise<void>) {
    if (!isTauri) return;
    const { getCurrentWindow } = await import("@tauri-apps/api/window");
    await cb(getCurrentWindow());
  }

  const winMinimize = () => withWin((w) => w.minimize());
  const winToggleMax = () => withWin((w) => w.toggleMaximize());
  const winClose = () => withWin((w) => w.close());

  async function syncMaximizedState() {
    if (!isTauri || !isWin) return null;
    await withWin(async (w) => {
      winMaximized.value = await w.isMaximized();
      return w.onResized(async () => {
        winMaximized.value = await w.isMaximized();
      });
    });
    return null;
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
