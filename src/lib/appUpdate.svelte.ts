// App updates: Lotus checks GitHub Releases (latest.json, signed) at startup and every 6 hours.
// When a newer version is out, a banner offers "Update and restart". The installer runs in
// passive mode (a small progress window, no questions) and the app restarts on the new version.

import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { getVersion } from "@tauri-apps/api/app";

const EVERY = 6 * 60 * 60 * 1000;

export const appUpdate = $state<{
  current: string | null;
  available: { version: string; notes: string | null } | null;
  checking: boolean;
  installing: boolean;
  /** 0–100 while downloading. */
  progress: number | null;
  /** The last manual check's outcome, for the Tools page. */
  message: string | null;
  dismissed: boolean;
}>({ current: null, available: null, checking: false, installing: false, progress: null, message: null, dismissed: false });

let pending: Update | null = null;

/** Checks for a newer version. `manual`: say what happened (the Tools button); otherwise quiet. */
export async function checkForUpdate(manual = false) {
  if (appUpdate.checking || appUpdate.installing) return;
  appUpdate.checking = true;
  if (manual) appUpdate.message = null;
  try {
    appUpdate.current ??= await getVersion();
    const update = await check();
    pending = update;
    appUpdate.available = update ? { version: update.version, notes: update.body ?? null } : null;
    if (update) appUpdate.dismissed = false;
    if (manual) appUpdate.message = update ? `Version ${update.version} is available.` : "You're on the latest version.";
  } catch (e) {
    // No release published yet, or offline: quiet unless asked.
    if (manual) {
      const raw = String(e).toLowerCase();
      appUpdate.message = /404|not found|json/.test(raw)
        ? "No releases published yet."
        : "Couldn't check for updates. Check your internet connection.";
    }
  } finally {
    appUpdate.checking = false;
  }
}

/** Downloads and installs the waiting update, then restarts Lotus. */
export async function installUpdate() {
  if (!pending || appUpdate.installing) return;
  appUpdate.installing = true;
  appUpdate.progress = 0;
  let total = 0;
  let done = 0;
  try {
    await pending.downloadAndInstall((event) => {
      if (event.event === "Started") total = event.data.contentLength ?? 0;
      else if (event.event === "Progress") {
        done += event.data.chunkLength;
        appUpdate.progress = total ? Math.min(100, Math.round((100 * done) / total)) : null;
      }
    });
    await relaunch();
  } catch (e) {
    appUpdate.installing = false;
    appUpdate.progress = null;
    appUpdate.message = "The update couldn't be installed. Try again, or download it from GitHub.";
  }
}

/** Starts the startup check and the 6-hourly checks (main window only). */
export function startUpdateChecks() {
  getVersion().then((v) => (appUpdate.current = v)).catch(() => {});
  checkForUpdate();
  const timer = setInterval(() => checkForUpdate(), EVERY);
  return () => clearInterval(timer);
}
