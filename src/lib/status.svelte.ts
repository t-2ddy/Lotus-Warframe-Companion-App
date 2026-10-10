// Pipeline status shared by the top bar and any page: game running, last capture, errors.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export type CaptureSummary = {
  capturedAtMs: number;
  bytes: number;
  address: string;
  regionKind: string;
  regions: number;
  scannedMb: number;
  durationMs: number;
  sections: [string, number][];
  savedTo: string;
};

export type Status = {
  gameRunning: boolean;
  logPath: string | null;
  lastEvent: string | null;
  lastCapture: CaptureSummary | null;
  lastError: string | null;
};

export const pipeline = $state<{ status: Status | null; capturing: boolean }>({
  status: null,
  capturing: false,
});

let started = false;

/** Loads the current status and keeps it updated. Safe to call more than once. */
export function startStatus() {
  if (started) return;
  started = true;
  invoke<Status>("get_status").then((s) => (pipeline.status = s));
  listen<Status>("pipeline-status", (e) => (pipeline.status = e.payload));
}

export async function captureNow() {
  pipeline.capturing = true;
  try {
    await invoke("capture_now");
  } catch {
    // The error is reported through pipeline-status.
  } finally {
    pipeline.capturing = false;
  }
}
