// Job progress composable: manages compress/extract progress, pause and cancel.

import { ref } from "vue";
import {
  cancelJob,
  pauseJob,
  resumeJob,
  type ProgressEvent,
} from "../lib/backend";

export type JobMode = "compress" | "extract";
export type Screen = "home" | "browser" | "compress" | "progress" | "done";

export function useJobProgress() {
  const mode = ref<JobMode>("compress");
  const jobId = ref<number | null>(null);
  const progress = ref<ProgressEvent | null>(null);
  const paused = ref(false);
  const returnScreen = ref<Screen>("home");
  let progressStartedAt = 0;

  function startJob(newMode: JobMode, newJobId: number, returnTo: Screen) {
    mode.value = newMode;
    jobId.value = newJobId;
    progress.value = null;
    paused.value = false;
    progressStartedAt = Date.now() / 1000;
    returnScreen.value = returnTo;
  }

  function updateProgress(e: ProgressEvent) {
    if (e.jobId === jobId.value) {
      progress.value = e;
    }
  }

  async function togglePause() {
    if (jobId.value == null) return;
    if (paused.value) await resumeJob(jobId.value);
    else await pauseJob(jobId.value);
    paused.value = !paused.value;
  }

  async function cancelCurrentJob(): Promise<Screen> {
    if (jobId.value != null) await cancelJob(jobId.value);
    jobId.value = null;
    progress.value = null;
    paused.value = false;
    return returnScreen.value;
  }

  function clearJob() {
    jobId.value = null;
    progress.value = null;
    paused.value = false;
  }

  function getProgressStartedAt() {
    return progressStartedAt;
  }

  return {
    mode,
    jobId,
    progress,
    paused,
    returnScreen,
    startJob,
    updateProgress,
    togglePause,
    cancelCurrentJob,
    clearJob,
    getProgressStartedAt,
  };
}
