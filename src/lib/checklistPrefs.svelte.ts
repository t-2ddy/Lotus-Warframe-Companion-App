// Which Home checklists are hidden (set on the Tools page). Kept in local storage, and shared as
// one state so Home and Tools agree without a reload.

export type ChecklistKind = "daily" | "weekly";

const KEY = "checklist.hidden";

function read(): Record<ChecklistKind, boolean> {
  const base = { daily: false, weekly: false };
  try {
    return { ...base, ...JSON.parse(localStorage.getItem(KEY) ?? "{}") };
  } catch {
    return base;
  }
}

export const checklistHidden = $state(read());

export function setChecklistHidden(kind: ChecklistKind, hidden: boolean) {
  checklistHidden[kind] = hidden;
  try {
    localStorage.setItem(KEY, JSON.stringify($state.snapshot(checklistHidden)));
  } catch {}
}
