// Shared overlay helper: fire a callback on Esc (closing the dialog). Together with ContextMenu's
// capture-phase Esc (which stops propagation), Esc closes only the menu, not the dialog beneath it.
import { onBeforeUnmount, onMounted } from "vue";

export function onEsc(cb: () => void) {
  const handler = (e: KeyboardEvent) => {
    if (e.key === "Escape") cb();
  };
  onMounted(() => document.addEventListener("keydown", handler));
  onBeforeUnmount(() => document.removeEventListener("keydown", handler));
}
