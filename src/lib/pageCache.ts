// The last data a page loaded, kept across page visits (memory) and app restarts (localStorage),
// so the page draws at once and refreshes in the background. A convenience only: anything here
// can be missing or a little old, and the page's own load replaces it.

const memory = new Map<string, unknown>();
const storageKey = (key: string) => `pageCache.${key}`;

export function cached<T>(key: string): T | null {
  if (memory.has(key)) return memory.get(key) as T;
  try {
    const saved = localStorage.getItem(storageKey(key));
    if (saved) {
      const value = JSON.parse(saved) as T;
      memory.set(key, value);
      return value;
    }
  } catch {}
  return null;
}

export function remember<T>(key: string, value: T) {
  memory.set(key, value);
  try {
    localStorage.setItem(storageKey(key), JSON.stringify(value));
  } catch {}
}
