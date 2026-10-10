// Small display helpers shared by pages.

/**
 * Item images are served by the app's local cache (src-tauri/src/images.rs).
 * On Windows, Tauri exposes custom protocols as http://<scheme>.localhost/.
 */
export const imageUrl = (name: string) => `http://wfimg.localhost/${encodeURIComponent(name)}`;

/** Marks an image that failed to load so CSS can show a placeholder tile instead. */
export const markMissing = (e: Event) => (e.currentTarget as HTMLImageElement).classList.add("missing");

/** Hides an image that failed to load (used for overlays, where a placeholder would cover the base image). */
export const hideOnError = (e: Event) => ((e.currentTarget as HTMLImageElement).style.display = "none");

export const signed = (n: number) => (n > 0 ? "+" : n < 0 ? "−" : "") + Math.abs(n).toLocaleString();

export const dateTime = (ms: number) =>
  new Date(ms).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });

export const time = (ms: number) => new Date(ms).toLocaleTimeString(undefined, { timeStyle: "short" });

/** Compact counts for small badges: 541, 2.5k, 3M. */
export const compact = (n: number) =>
  new Intl.NumberFormat(undefined, { notation: "compact", maximumFractionDigits: 1 }).format(n);
