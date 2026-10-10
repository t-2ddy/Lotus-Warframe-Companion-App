/**
 * Plain-language versions of the errors the backend passes up (network library text such as
 * "io: dns error: …" or "http status: 503"). `source` names what was being reached, e.g.
 * "warframe.market".
 */
export function friendlyError(e: unknown, source = "the server"): string {
  const raw = String(e ?? "").toLowerCase();
  const offline = typeof navigator !== "undefined" && navigator.onLine === false;
  if (
    offline ||
    /dns|resolve|lookup|no such host|network is unreachable|connection refused|connection reset|failed to connect|os error 10051|os error 11001/.test(raw)
  ) {
    return "No internet connection.";
  }
  if (/timed? ?out|timeout/.test(raw)) return `${capitalize(source)} took too long to answer.`;
  if (/429|too many requests/.test(raw)) return `Too many requests to ${source}. Trying again shortly.`;
  if (/\b5\d\d\b|bad gateway|service unavailable|gateway timeout/.test(raw)) return `${capitalize(source)} isn't responding right now.`;
  if (/\b404\b|not found/.test(raw)) return `${capitalize(source)} doesn't have that right now.`;
  if (/database unavailable/.test(raw)) return "The app's local database couldn't be opened. Try restarting Lotus.";
  if (/no account captured|no inventory/.test(raw)) return "No inventory captured yet. Log in to Warframe with Lotus open.";
  return `Something went wrong reaching ${source}. Try again in a bit.`;
}

/** "the server" → "The server" at the start of a sentence; names like "warframe.market" stay as they are. */
const capitalize = (s: string) => (s.startsWith("the ") ? "The" + s.slice(3) : s);
