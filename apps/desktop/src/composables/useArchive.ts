// Archive operations composable: open, list, password management.

import { computed, ref } from "vue";
import { ArchiveIndex } from "../lib/archiveTree";
import {
  listArchive,
  fileName,
  type EntryInfo,
} from "../lib/backend";
import { useToast } from "./useToast";

export function useArchive() {
  const { toastErr } = useToast();

  const archivePath = ref("");
  const entries = ref<EntryInfo[]>([]);
  const listLoading = ref(false);
  const needsPassword = ref(false);
  const password = ref("");

  const index = computed(() => new ArchiveIndex(entries.value));
  const archiveName = computed(() => fileName(archivePath.value));
  const parentOf = (p: string) => p.slice(0, p.length - fileName(p).length);

  async function openArchive(path: string, pw?: string) {
    archivePath.value = path;
    password.value = pw ?? "";
    entries.value = [];
    needsPassword.value = false;
    listLoading.value = true;
    try {
      const list = await listArchive(path, "auto", pw || undefined);
      entries.value = list;
      needsPassword.value = false;
    } catch (e) {
      entries.value = [];
      needsPassword.value = (e as { kind?: string }).kind === "password";
      if (!needsPassword.value) toastErr(e);
    } finally {
      listLoading.value = false;
    }
  }

  function resetArchive() {
    entries.value = [];
    archivePath.value = "";
    password.value = "";
    needsPassword.value = false;
  }

  return {
    archivePath,
    entries,
    listLoading,
    needsPassword,
    password,
    index,
    archiveName,
    parentOf,
    openArchive,
    resetArchive,
  };
}
