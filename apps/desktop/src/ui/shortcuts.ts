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

/** A shortcut as shown in Settings: the keys pressed and what they do. */
export interface ShortcutHelp {
  action: ShortcutAction;
  label: string;
  /** The keys, as a key event. */
  keys: KeyLike;
}

const key = (k: string, mods: Partial<KeyLike> = {}): KeyLike => ({
  key: k,
  ctrlKey: false,
  metaKey: false,
  shiftKey: false,
  altKey: false,
  ...mods,
});

/** The application shortcuts for this platform, for the Settings list. */
export function shortcutHelp(mac: boolean): ShortcutHelp[] {
  const mod = (k: string, code?: string): KeyLike =>
    mac ? key(k, { metaKey: true, code }) : key(k.toUpperCase(), { ctrlKey: true, shiftKey: true, code });
  const list: ShortcutHelp[] = [
    { action: "newTab", label: "New local terminal", keys: mod("t") },
    { action: "closeTab", label: "Close tab", keys: mod("w") },
    { action: "nextTab", label: "Next tab", keys: key("Tab", { ctrlKey: true }) },
    { action: "prevTab", label: "Previous tab", keys: key("Tab", { ctrlKey: true, shiftKey: true }) },
    { action: "tab1", label: "Go to tab 1–8 (9 = last)", keys: mod("1", "Digit1") },
    { action: "palette", label: "Connect by typing (palette)", keys: mod("k") },
    { action: "find", label: "Find in terminal", keys: mod("f") },
    { action: "zoomIn", label: "Larger text", keys: mod("=") },
    { action: "zoomOut", label: "Smaller text", keys: mod("-") },
    { action: "zoomReset", label: "Default text size", keys: mod("0") },
  ];
  if (!mac) {
    list.splice(6, 0, { action: "copy", label: "Copy", keys: mod("c") }, { action: "paste", label: "Paste", keys: mod("v") });
  }
  return list;
}

/** Key names for display: ["⌘", "T"] on macOS, ["Ctrl", "Shift", "T"] elsewhere. */
export function keyCaps(e: KeyLike, mac: boolean): string[] {
  const caps: string[] = [];
  if (e.ctrlKey) caps.push(mac ? "⌃" : "Ctrl");
  if (e.altKey) caps.push(mac ? "⌥" : "Alt");
  if (e.shiftKey) caps.push(mac ? "⇧" : "Shift");
  if (e.metaKey) caps.push("⌘");
  caps.push(e.key.length === 1 ? e.key.toUpperCase() : e.key);
  return caps;
}
