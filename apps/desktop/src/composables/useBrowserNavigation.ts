// Browser navigation composable: manages directory history and back/forward/up navigation.
import { computed, ref } from "vue";

export function useBrowserNavigation() {
  const history = ref<string[]>([""]);
  const hIndex = ref(0);

  const dir = computed(() => history.value[hIndex.value] ?? "");
  const canBack = computed(() => hIndex.value > 0);
  const canForward = computed(() => hIndex.value < history.value.length - 1);
  const canUp = computed(() => dir.value !== "");

  function cd(target: string) {
    if (target === dir.value) return;
    history.value = history.value.slice(0, hIndex.value + 1);
    history.value.push(target);
    hIndex.value++;
  }

  function back() {
    if (canBack.value) hIndex.value--;
  }

  function forward() {
    if (canForward.value) hIndex.value++;
  }

  function up() {
    if (!canUp.value) return;
    cd(dir.value.includes("/") ? dir.value.slice(0, dir.value.lastIndexOf("/")) : "");
  }

  function reset() {
    history.value = [""];
    hIndex.value = 0;
  }

  return {
    dir,
    canBack,
    canForward,
    canUp,
    cd,
    back,
    forward,
    up,
    reset,
  };
}
