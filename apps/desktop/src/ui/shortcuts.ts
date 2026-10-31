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
  | "zoomReset";

export interface KeyLike {
  key: string;
  ctrlKey: boolean;
  metaKey: boolean;
  shiftKey: boolean;
  altKey: boolean;
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
  const k = e.key.length === 1 ? e.key.toLowerCase() : e.key;
  switch (k) {
    case "t":
      return "newTab";
    case "w":
      return "closeTab";
    case "f":
      return "find";
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
