// Settings composable: manages default format, presets, exclude-hidden-files and related settings.

import { ref } from "vue";

export function useSettings() {
  const defFormat = ref(localStorage.getItem("wispzip.def.format") ?? "zip");
  const defPreset = ref(localStorage.getItem("wispzip.def.preset") ?? "std");
  const defExcl = ref(localStorage.getItem("wispzip.def.excl") === "1");

  function saveSettings(v: { defFormat: string; defPreset: string; defExcludeHidden: boolean }) {
    defFormat.value = v.defFormat;
    defPreset.value = v.defPreset;
    defExcl.value = v.defExcludeHidden;
    localStorage.setItem("wispzip.def.format", v.defFormat);
    localStorage.setItem("wispzip.def.preset", v.defPreset);
    localStorage.setItem("wispzip.def.excl", v.defExcludeHidden ? "1" : "0");
  }

  function getExcludeHidden(): boolean {
    return localStorage.getItem("wispzip.def.excl") === "1";
  }

  function getFormat(): string {
    return localStorage.getItem("wispzip.def.format") ?? "zip";
  }

  return {
    defFormat,
    defPreset,
    defExcl,
    saveSettings,
    getExcludeHidden,
    getFormat,
  };
}
