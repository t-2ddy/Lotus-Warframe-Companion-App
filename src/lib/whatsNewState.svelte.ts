// The "What's new" popup: shown once after the app updates, with the bug-fix notes of every
// version since the one last seen (remembered in local storage). Not on a new install: the
// welcome screen covers that. Reopenable from the Tools page.

import { getVersion } from "@tauri-apps/api/app";
import { RELEASE_NOTES, type ReleaseNote } from "./releaseNotes";

const KEY = "whatsNew.seenVersion";

export const whatsNew = $state<{ open: boolean; version: string | null; notes: ReleaseNote[] }>({
  open: false,
  version: null,
  notes: [],
});

function read(): string | null {
  try {
    return localStorage.getItem(KEY);
  } catch {
    return null;
  }
}

function remember(version: string | null) {
  if (!version) return;
  try {
    localStorage.setItem(KEY, version);
  } catch {}
}

/** Compares "x.y.z" versions (a pre-release tag is ignored). */
function compare(a: string, b: string): number {
  const parts = (v: string) => v.split("-")[0].split(".").map((n) => Number(n) || 0);
  const [pa, pb] = [parts(a), parts(b)];
  for (let i = 0; i < 3; i++) {
    if ((pa[i] ?? 0) !== (pb[i] ?? 0)) return (pa[i] ?? 0) - (pb[i] ?? 0);
  }
  return 0;
}

/** At startup. `newInstall`: the welcome screen is showing, so this version's notes aren't news. */
export async function startWhatsNew(newInstall: boolean) {
  const current = await getVersion().catch(() => null);
  if (!current) return;
  whatsNew.version = current;
  const seen = read();
  if (newInstall) {
    remember(current);
    return;
  }
  // Updated since the last version seen; from before this popup existed, just this version.
  const notes = RELEASE_NOTES.filter(
    (n) =>
      n.fixes.length > 0 &&
      compare(n.version, current) <= 0 &&
      (seen ? compare(n.version, seen) > 0 : compare(n.version, current) === 0),
  );
  if (notes.length > 0) {
    whatsNew.notes = notes;
    whatsNew.open = true;
  } else {
    remember(current);
  }
}

export function closeWhatsNew() {
  whatsNew.open = false;
  remember(whatsNew.version);
}

/** From the Tools page: every version's notes. */
export function openWhatsNew() {
  whatsNew.notes = RELEASE_NOTES.filter((n) => n.fixes.length > 0);
  whatsNew.open = whatsNew.notes.length > 0;
}
