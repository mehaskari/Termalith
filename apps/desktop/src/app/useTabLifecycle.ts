import { type Dispatch, type MutableRefObject, type SetStateAction, useEffect, useRef, useState } from "react";
import { type DesktopInfo, closeDesktop, closeTerminal, listDesktops } from "../terminal/api";
import type { Tab } from "./tabs";

/** The ids in `before` that are no longer in `now`. */
export function released<T>(before: Set<T>, now: Set<T>): T[] {
  return [...before].filter((id) => !now.has(id));
}

/** Tabs with the desktop states from `list`; the same array when nothing changed. */
export function withDesktopStates(tabs: Tab[], list: DesktopInfo[]): Tab[] {
  let changed = false;
  const next = tabs.map((t) => {
    const d = t.desktop && list.find((x) => x.id === t.desktop?.id);
    if (d && d.kind !== t.desktop?.kind) {
      changed = true;
      return { ...t, desktop: d };
    }
    return t;
  });
  return changed ? next : tabs;
}

/**
 * What happens to tabs over time without the user: external desktop clients
 * are watched, closed tabs release their connections and clients, dropped
 * SSH tabs reconnect after their countdown, and showing a tab clears its
 * new-output dot. Returns the clock the reconnect countdown is drawn with.
 */
export function useTabLifecycle(opts: {
  tabs: Tab[];
  setTabs: Dispatch<SetStateAction<Tab[]>>;
  tabsRef: MutableRefObject<Tab[]>;
  active: number | null;
  patchTab: (key: number, patch: Partial<Tab>) => void;
  closeTab: (key: number) => void;
}): number {
  const { tabs, setTabs, tabsRef, active, patchTab, closeTab } = opts;

  // Remote desktop status: poll while a client is running.
  const watchDesktops = tabs.some((t) => t.desktop?.kind === "running");
  useEffect(() => {
    if (!watchDesktops) return;
    const timer = window.setInterval(() => {
      listDesktops().then(
        (list) => setTabs((ts) => withDesktopStates(ts, list)),
        () => {},
      );
    }, 2000);
    return () => window.clearInterval(timer);
  }, [watchDesktops, setTabs]);

  // Closing a file browser tab releases its SSH connection.
  const openFileIds = useRef(new Set<number>());
  useEffect(() => {
    const now = new Set(tabs.flatMap((t) => (t.kind === "files" && t.terminalId !== undefined ? [t.terminalId] : [])));
    for (const id of released(openFileIds.current, now)) void closeTerminal(id).catch(() => {});
    openFileIds.current = now;
  }, [tabs]);

  // Closing a desktop tab (by any route) stops its client and tunnel.
  const openDesktopIds = useRef(new Set<number>());
  useEffect(() => {
    const now = new Set(tabs.flatMap((t) => (t.desktop ? [t.desktop.id] : [])));
    for (const id of released(openDesktopIds.current, now)) void closeDesktop(id).catch(() => {});
    openDesktopIds.current = now;
  }, [tabs]);

  // Automatic reconnect of dropped SSH tabs, with a visible countdown.
  const [clock, setClock] = useState(() => Date.now());
  const pendingReconnect = tabs.some((t) => t.reconnectAt !== undefined);
  useEffect(() => {
    if (!pendingReconnect) return;
    const timer = window.setInterval(() => setClock(Date.now()), 500);
    return () => window.clearInterval(timer);
  }, [pendingReconnect]);
  useEffect(() => {
    for (const t of tabs) {
      if (t.reconnectAt !== undefined && clock >= t.reconnectAt && t.reconnect) {
        const again = t.reconnect;
        closeTab(t.key);
        again();
      }
    }
  }, [clock, tabs, closeTab]);

  // Showing a tab clears its new-output dot.
  useEffect(() => {
    if (active !== null && tabsRef.current.find((t) => t.key === active)?.activity) {
      patchTab(active, { activity: false });
    }
  }, [active, patchTab, tabsRef]);

  return clock;
}
