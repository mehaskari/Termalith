// Keyboard handling shared by the app's floating menus (WAI-ARIA menu
// pattern, simplified): arrows, Home and End move between enabled items and
// wrap; Esc and Tab close the menu.

/** Focuses the first enabled item of `menu`. */
export function focusFirstItem(menu: HTMLElement | null): void {
  menu?.querySelector<HTMLButtonElement>("button:not(:disabled)")?.focus();
}

/** Index to focus after `key`, or null if the key is not a move key. */
export function nextIndex(key: string, current: number, count: number): number | null {
  if (count === 0) return null;
  const to: Record<string, number> = { ArrowDown: current + 1, ArrowUp: current - 1, Home: 0, End: count - 1 };
  const i = to[key];
  return i === undefined ? null : (i + count) % count;
}

/** onKeyDown for a menu element. */
export function menuKeyDown(e: React.KeyboardEvent<HTMLElement>, close: () => void): void {
  const items = [...e.currentTarget.querySelectorAll<HTMLButtonElement>("button:not(:disabled)")];
  const i = nextIndex(e.key, items.indexOf(document.activeElement as HTMLButtonElement), items.length);
  if (i !== null) {
    e.preventDefault();
    items[i]?.focus();
  } else if (e.key === "Escape" || e.key === "Tab") {
    e.preventDefault();
    close();
  }
}
