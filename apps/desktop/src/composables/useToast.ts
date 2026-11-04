// Toast composable: manages global messages, with a busy state and auto-dismiss.

import { ref } from "vue";
import { t } from "../lib/i18n";

const toastMsg = ref("");
const toastBusy = ref(false);
let toastTimer = 0;

export function useToast() {
  function toast(msg: string, busy = false) {
    toastMsg.value = msg;
    toastBusy.value = busy;
    window.clearTimeout(toastTimer);
    if (!busy) toastTimer = window.setTimeout(() => (toastMsg.value = ""), 3600);
  }

  function toastErr(e: unknown) {
    const err = e as { kind?: string; message?: string };
    if (err?.kind === "cancelled") return toast(t("toast.cancelled"));
    if (err?.kind === "password") return toast(t("toast.password"));
    if (err?.kind === "unsafe_entry") return toast(t("toast.unsafe"));
    toast(err?.message ?? String(e ?? t("toast.failed")));
  }

  function clearToast() {
    toastMsg.value = "";
    toastBusy.value = false;
  }

  return {
    toastMsg,
    toastBusy,
    toast,
    toastErr,
    clearToast,
  };
}
