// Browser sort composable: manages sort state and sort logic.
import { ref } from "vue";

export type SortKey = "name" | "size" | "type" | "mtime";

export function useBrowserSort() {
  const sortKey = ref<SortKey>("name");
  const sortAsc = ref(true);

  function setSort(k: SortKey) {
    if (sortKey.value === k) sortAsc.value = !sortAsc.value;
    else {
      sortKey.value = k;
      sortAsc.value = true;
    }
  }

  return {
    sortKey,
    sortAsc,
    setSort,
  };
}
