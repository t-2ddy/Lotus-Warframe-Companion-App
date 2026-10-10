// The first-run welcome screen: open until dismissed once (remembered in local storage), and
// reopenable from the Tools page.

const KEY = "welcome.seen";

function seen(): boolean {
  try {
    return localStorage.getItem(KEY) === "1";
  } catch {
    return false;
  }
}

export const welcome = $state({ open: !seen() });

export function closeWelcome() {
  welcome.open = false;
  try {
    localStorage.setItem(KEY, "1");
  } catch {}
}

export function openWelcome() {
  welcome.open = true;
}
