import type { ITheme } from "@xterm/xterm";
import type { Settings } from "../terminal/api";

export type ResolvedTheme = "dark" | "light";

export function resolveTheme(pref: Settings["theme"], systemDark: boolean): ResolvedTheme {
  if (pref === "system") return systemDark ? "dark" : "light";
  return pref;
}

export const TERMINAL_THEMES: Record<ResolvedTheme, ITheme> = {
  dark: {
    background: "#101317",
    foreground: "#d7dce3",
    cursor: "#7fd1b9",
    selectionBackground: "#2c3a46",
  },
  light: {
    background: "#fbfbf9",
    foreground: "#1d232b",
    cursor: "#1f7a63",
    cursorAccent: "#fbfbf9",
    selectionBackground: "#cfe3ff",
    black: "#1d232b",
    red: "#b42318",
    green: "#1f7a3a",
    yellow: "#8a6100",
    blue: "#1d4fd8",
    magenta: "#8b2fc9",
    cyan: "#0e7490",
    white: "#6b7280",
    brightBlack: "#4b5563",
    brightWhite: "#111827",
  },
};

/** Built-in terminal color schemes (open palettes). "app" follows the app theme. */
export const TERMINAL_SCHEMES: { id: string; name: string; theme?: ITheme }[] = [
  { id: "app", name: "Termalith (follows app theme)" },
  {
    id: "solarized-dark",
    name: "Solarized Dark",
    theme: {
      background: "#002b36", foreground: "#839496", cursor: "#93a1a1", selectionBackground: "#073642",
      black: "#073642", red: "#dc322f", green: "#859900", yellow: "#b58900",
      blue: "#268bd2", magenta: "#d33682", cyan: "#2aa198", white: "#eee8d5",
      brightBlack: "#586e75", brightRed: "#cb4b16", brightGreen: "#586e75", brightYellow: "#657b83",
      brightBlue: "#839496", brightMagenta: "#6c71c4", brightCyan: "#93a1a1", brightWhite: "#fdf6e3",
    },
  },
  {
    id: "solarized-light",
    name: "Solarized Light",
    theme: {
      background: "#fdf6e3", foreground: "#657b83", cursor: "#586e75", cursorAccent: "#fdf6e3",
      selectionBackground: "#eee8d5",
      black: "#073642", red: "#dc322f", green: "#859900", yellow: "#b58900",
      blue: "#268bd2", magenta: "#d33682", cyan: "#2aa198", white: "#eee8d5",
      brightBlack: "#002b36", brightRed: "#cb4b16", brightGreen: "#586e75", brightYellow: "#657b83",
      brightBlue: "#839496", brightMagenta: "#6c71c4", brightCyan: "#93a1a1", brightWhite: "#fdf6e3",
    },
  },
  {
    id: "nord",
    name: "Nord",
    theme: {
      background: "#2e3440", foreground: "#d8dee9", cursor: "#d8dee9", selectionBackground: "#434c5e",
      black: "#3b4252", red: "#bf616a", green: "#a3be8c", yellow: "#ebcb8b",
      blue: "#81a1c1", magenta: "#b48ead", cyan: "#88c0d0", white: "#e5e9f0",
      brightBlack: "#4c566a", brightRed: "#bf616a", brightGreen: "#a3be8c", brightYellow: "#ebcb8b",
      brightBlue: "#81a1c1", brightMagenta: "#b48ead", brightCyan: "#8fbcbb", brightWhite: "#eceff4",
    },
  },
  {
    id: "gruvbox-dark",
    name: "Gruvbox Dark",
    theme: {
      background: "#282828", foreground: "#ebdbb2", cursor: "#ebdbb2", selectionBackground: "#504945",
      black: "#282828", red: "#cc241d", green: "#98971a", yellow: "#d79921",
      blue: "#458588", magenta: "#b16286", cyan: "#689d6a", white: "#a89984",
      brightBlack: "#928374", brightRed: "#fb4934", brightGreen: "#b8bb26", brightYellow: "#fabd2f",
      brightBlue: "#83a598", brightMagenta: "#d3869b", brightCyan: "#8ec07c", brightWhite: "#ebdbb2",
    },
  },
  {
    id: "tomorrow-night",
    name: "Tomorrow Night",
    theme: {
      background: "#1d1f21", foreground: "#c5c8c6", cursor: "#c5c8c6", selectionBackground: "#373b41",
      black: "#1d1f21", red: "#cc6666", green: "#b5bd68", yellow: "#f0c674",
      blue: "#81a2be", magenta: "#b294bb", cyan: "#8abeb7", white: "#c5c8c6",
      brightBlack: "#969896", brightRed: "#cc6666", brightGreen: "#b5bd68", brightYellow: "#f0c674",
      brightBlue: "#81a2be", brightMagenta: "#b294bb", brightCyan: "#8abeb7", brightWhite: "#ffffff",
    },
  },
];

/** The xterm theme for a scheme id; unknown ids and "app" follow the app theme. */
export function terminalTheme(scheme: string, resolved: ResolvedTheme): ITheme {
  return TERMINAL_SCHEMES.find((s) => s.id === scheme)?.theme ?? TERMINAL_THEMES[resolved];
}

export const DEFAULT_FONT = 'Menlo, "SF Mono", Monaco, Consolas, "DejaVu Sans Mono", monospace';

export function fontStack(family: string): string {
  const f = family.trim();
  return f ? `${f.includes(" ") && !f.includes(",") ? `"${f}"` : f}, ${DEFAULT_FONT}` : DEFAULT_FONT;
}
