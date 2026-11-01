// Application keyboard shortcuts. On macOS they use Cmd; elsewhere
// Ctrl+Shift, because plain Ctrl+W/T/C/V/F mean something to programs running
// in the terminal (delete word, transpose, interrupt, …).

export type ShortcutAction =
  | "newTab"
  | "closeTab"
  | "nextTab"
  | "prevTab"
  | "find"
  | "copy"
  | "paste"
  | "zoomIn"
  | "zoomOut"
  | "zoomReset"
  | "palette"
  /** Go to tab N (1-9; 9 = last tab). */
  | `tab${1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9}`;

export interface KeyLike {
  key: string;
  ctrlKey: boolean;
  metaKey: boolean;
  shiftKey: boolean;
  altKey: boolean;
  /** Physical key (e.g. "Digit1"); Shift+digit changes `key` to a symbol. */
  code?: string;
}

export function isMacPlatform(): boolean {
  return typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.platform || navigator.userAgent);
}

export function matchShortcut(e: KeyLike, mac: boolean): ShortcutAction | null {
  if (e.altKey) return null;
  // Tab switching is Ctrl(+Shift)+Tab on every platform.
  if (e.ctrlKey && !e.metaKey && e.key === "Tab") return e.shiftKey ? "prevTab" : "nextTab";

  const mod = mac ? e.metaKey && !e.ctrlKey : e.ctrlKey && e.shiftKey && !e.metaKey;
  if (!mod) return null;
  const digit = /^Digit([1-9])$/.exec(e.code ?? "")?.[1] ?? (/^[1-9]$/.test(e.key) ? e.key : null);
  if (digit) return `tab${Number(digit) as 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9}`;
  const k = e.key.length === 1 ? e.key.toLowerCase() : e.key;
  switch (k) {
    case "t":
      return "newTab";
    case "w":
      return "closeTab";
    case "f":
      return "find";
    case "k":
      return "palette";
    case "c":
      return mac ? null : "copy"; // Cmd+C is the native copy on macOS
    case "v":
      return mac ? null : "paste";
    case "=":
    case "+":
      return "zoomIn";
    case "-":
    case "_":
      return "zoomOut";
    case "0":
    case ")":
      return "zoomReset";
    default:
      return null;
  }
}
