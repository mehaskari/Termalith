/**
 * Recently opened saved sessions, newest first: a per-device convenience
 * for the home screen, kept in localStorage (ids only, no host names).
 */
const KEY = "termalith.recentSessions";
export const MAX_RECENT = 8;

export function readRecent(storage: Pick<Storage, "getItem"> | undefined = globalThis.localStorage): string[] {
  try {
    const raw = storage?.getItem(KEY);
    const list: unknown = raw ? JSON.parse(raw) : [];
    return Array.isArray(list) ? list.filter((x): x is string => typeof x === "string").slice(0, MAX_RECENT) : [];
  } catch {
    return [];
  }
}

/** `id` first, without duplicates, at most MAX_RECENT. */
export function withRecent(list: string[], id: string): string[] {
  return [id, ...list.filter((x) => x !== id)].slice(0, MAX_RECENT);
}

export function rememberRecent(id: string, storage: Pick<Storage, "getItem" | "setItem"> | undefined = globalThis.localStorage) {
  try {
    storage?.setItem(KEY, JSON.stringify(withRecent(readRecent(storage), id)));
  } catch {
    // Private mode or blocked storage: the list is only a convenience.
  }
}
