// Browser search composable: manages search state and debouncing.
import { computed, ref, watch } from "vue";

export function useBrowserSearch() {
  const search = ref("");
  const debouncedSearch = ref("");
  let searchTimer = 0;

  // Search debounce: 300ms, avoiding repeated filtering when there are many files
  watch(search, (v) => {
    clearTimeout(searchTimer);
    searchTimer = window.setTimeout(() => {
      debouncedSearch.value = v;
    }, 300);
  });

  const searching = computed(() => debouncedSearch.value.trim().length > 0);

  function clear() {
    search.value = "";
    debouncedSearch.value = "";
  }

  return {
    search,
    debouncedSearch,
    searching,
    clear,
  };
}
