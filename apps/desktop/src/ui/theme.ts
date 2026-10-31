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

export const DEFAULT_FONT = 'Menlo, "SF Mono", Monaco, Consolas, "DejaVu Sans Mono", monospace';

export function fontStack(family: string): string {
  const f = family.trim();
  return f ? `${f.includes(" ") && !f.includes(",") ? `"${f}"` : f}, ${DEFAULT_FONT}` : DEFAULT_FONT;
}
