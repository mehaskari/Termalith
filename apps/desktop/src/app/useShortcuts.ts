import { useEffect, useRef } from "react";
import { type Settings, saveSettings } from "../terminal/api";
import { isMacPlatform, matchShortcut } from "../ui/shortcuts";
import type { Tab } from "./tabs";

export interface ShortcutHandlers {
  tabs: Tab[];
  active: number | null;
  settings: Settings;
  setActive: (key: number | null) => void;
  setSettings: (s: Settings) => void;
  openLocal: () => void;
  requestClose: (key: number) => void;
  openPalette: () => void;
  patchTab: (key: number, patch: Partial<Tab>) => void;
}

/** The new font size for a zoom shortcut (8–32, reset to 13). */
export function zoomedFontSize(current: number, action: "zoomIn" | "zoomOut" | "zoomReset"): number {
  if (action === "zoomReset") return 13;
  return Math.min(32, Math.max(8, current + (action === "zoomIn" ? 1 : -1)));
}

/** The tab a "tabN" shortcut selects: like browsers, 9 is the last tab. */
export function nthTab<T>(tabs: T[], n: number): T | undefined {
  return n === 9 ? tabs[tabs.length - 1] : tabs[n - 1];
}

/**
 * Keyboard shortcuts, captured before the terminal sees the key. Open
 * dialogs keep their keys. Terminal actions (copy, find, …) go to the active
 * tab as a command.
 */
export function useShortcuts(handlers: ShortcutHandlers) {
  const current = useRef(handlers);
  current.current = handlers;
  useEffect(() => {
    const mac = isMacPlatform();
    const onKey = (e: KeyboardEvent) => {
      const action = matchShortcut(e, mac);
      if (!action) return;
      if (document.querySelector("dialog[open]")) return;
      e.preventDefault();
      e.stopPropagation();
      const h = current.current;
      const { tabs: ts, active: a } = h;
      const i = ts.findIndex((t) => t.key === a);
      switch (action) {
        case "newTab":
          h.openLocal();
          break;
        case "closeTab":
          if (a !== null) h.requestClose(a);
          break;
        case "nextTab":
        case "prevTab":
          if (ts.length) {
            const n = (i + (action === "nextTab" ? 1 : -1) + ts.length) % ts.length;
            h.setActive(ts[n]?.key ?? null);
          }
          break;
        case "zoomIn":
        case "zoomOut":
        case "zoomReset": {
          const next = { ...h.settings, fontSize: zoomedFontSize(h.settings.fontSize, action) };
          h.setSettings(next);
          void saveSettings(next).catch(() => {});
          break;
        }
        case "palette":
          h.openPalette();
          break;
        case "tab1":
        case "tab2":
        case "tab3":
        case "tab4":
        case "tab5":
        case "tab6":
        case "tab7":
        case "tab8":
        case "tab9": {
          const t = nthTab(ts, Number(action.slice(3)));
          if (t) h.setActive(t.key);
          break;
        }
        default:
          if (a !== null) h.patchTab(a, { command: { seq: performance.now(), action } });
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, []);
}
