import { useCallback, useRef, useState } from "react";
import {
  type DesktopInfo,
  type LogInfo,
  type Opener,
  type TerminalId,
  splitTerminal,
} from "../terminal/api";
import type { DiffTarget } from "../ui/DiffPane";

/** Panes in a tab split as a grid, the first terminal included. */
const GRID_PANES = 4;

export interface SplitPane {
  key: number;
  open: Opener;
}

export interface Tab {
  key: number;
  title: string;
  kind: "local" | "ssh" | "serial" | "tools" | "desktop" | "editor" | "telnet" | "files" | "diff" | "rdp";
  /** A files tab on a protocol without terminals (FTP). */
  noShell?: boolean;
  /** The Files panel follows the folder of this tab's shell. */
  followFolder?: boolean;
  /** The folder and host the shell last reported (OSC 7). */
  shellFolder?: { host: string; path: string };
  open: Opener;
  terminalId?: TerminalId;
  /** Connection over (failed, or the remote shell exited). */
  ended?: boolean;
  filesOpen?: boolean;
  /** Latest shortcut command for the main terminal of this tab. */
  command?: { seq: number; action: "find" | "copy" | "paste" | "save" };
  tunnelsOpen?: boolean;
  snippetsOpen?: boolean;
  /** Set when the connection was refused because the host key changed. */
  changedKey?: { host: string; port: number; presented: string; known: string[] };
  reconnect?: () => void;
  /** Extra panes beside the main terminal. */
  splits: SplitPane[];
  direction: "row" | "column" | "grid";
  /** Remote desktop tabs: the external client session (null while opening). */
  desktop?: DesktopInfo | null;
  desktopError?: string | null;
  sessionId?: string;
  /** Active session log of the main terminal. */
  log?: LogInfo | null;
  /** Editor tabs: the remote file and the SSH terminal whose connection is used. */
  edit?: { terminalId: TerminalId; dir: string; name: string };
  /** Editor has unsaved changes. */
  dirty?: boolean;
  /** Diff tabs: the two files compared. */
  diff?: { left: DiffTarget; right: DiffTarget };
  /** Connection lost: time (ms since epoch) of the automatic reconnect. */
  reconnectAt?: number;
  /** Local tabs: the shell id they were opened with (null = default). */
  shellId?: string | null;
  /** Command typed into the shell once it is ready (saved session option). */
  startup?: string;
  /** New output arrived while the tab was not shown. */
  activity?: boolean;
}

/**
 * The open tabs and which one is shown. `tabsRef` always holds the latest
 * list for callbacks that must not re-subscribe; `nextKey` numbers tabs and
 * split panes.
 */
export function useTabs() {
  const [tabs, setTabs] = useState<Tab[]>([]);
  const [active, setActive] = useState<number | null>(null);
  const tabsRef = useRef<Tab[]>([]);
  tabsRef.current = tabs;
  const nextKey = useRef(1);

  const addTab = useCallback((title: string, kind: Tab["kind"], open: Opener) => {
    const key = nextKey.current++;
    setTabs((t) => [...t, { key, title, kind, open, splits: [], direction: "row" }]);
    setActive(key);
    return key;
  }, []);

  const renameTab = useCallback((key: number, title: string) => {
    setTabs((t) => t.map((x) => (x.key === key ? { ...x, title } : x)));
  }, []);

  const patchTab = useCallback((key: number, patch: Partial<Tab>) => {
    setTabs((t) => t.map((x) => (x.key === key ? { ...x, ...patch } : x)));
  }, []);

  const splitTab = useCallback((tab: Tab, direction: Tab["direction"]) => {
    if (tab.terminalId === undefined) return;
    const source = tab.terminalId;
    // A grid fills up to four panes; a row or column adds one.
    const added = direction === "grid" ? Math.max(0, GRID_PANES - 1 - tab.splits.length) : 1;
    const panes = Array.from({ length: added }, () => ({ key: nextKey.current++, open: splitTerminal(source) }));
    setTabs((t) =>
      t.map((x) => (x.key === tab.key ? { ...x, direction, splits: [...x.splits, ...panes] } : x)),
    );
  }, []);

  const closeSplit = useCallback((tabKey: number, paneKey: number) => {
    setTabs((t) =>
      t.map((x) => (x.key === tabKey ? { ...x, splits: x.splits.filter((s) => s.key !== paneKey) } : x)),
    );
  }, []);

  const closeTab = useCallback((key: number) => {
    setTabs((t) => {
      const i = t.findIndex((x) => x.key === key);
      const rest = t.filter((x) => x.key !== key);
      setActive((a) => (a === key ? (rest[Math.max(0, i - 1)]?.key ?? null) : a));
      return rest;
    });
  }, []);

  return { tabs, setTabs, tabsRef, active, setActive, nextKey, addTab, renameTab, patchTab, splitTab, closeSplit, closeTab };
}
