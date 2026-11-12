import { Fragment, useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { TerminalView } from "./terminal/TerminalView";
import {
  type AppInfo,
  DEFAULT_SETTINGS,
  type Settings,
  checkForUpdates,
  getSettings,
  saveSettings,
  setFavorite,
  writeTerminal,
  type MacroStep,
  createMacro,
  type DroppedFile,
  type IpcError,
  type TerminalId,
  type HostKeyAnswer,
  type HostKeyPrompt,
  type Opener,
  type SessionInput,
  type SessionRecord,
  type SshTarget,
  appInfo,
  listFolders,
  listFolderIcons,
  setFolderIcon,
  type SessionIcon,
  createFolder,
  renameFolder,
  deleteFolder,
  moveSession,
  type CrashReport,
  crashReports,
  dismissCrashReports,
  createSession,
  deleteSession,
  forgetHostKey,
  forgetSessionSecret,
  isIpcError,
  listSessions,
  openLocalTerminal,
  moshTerminal,
  openSsh,
  openSerial,
  onLocalNetworkBlocked,
  respondHostKey,
  serialSummary,
  splitTerminal,
  updateSession,
  type AuthPrompt,
  respondAuthPrompt,
  startLog,
  stopLog,
  closeDesktop,
  telnetTerminal,
  openFtpFiles,
  openSshFiles,
  closeTerminal,
  type ShellChoice,
  listShells,
  localTerminal,
  listDesktops,
  openDesktop,
  desktopClients,
} from "./terminal/api";
import { DesktopPane } from "./ui/DesktopPane";
import { EditorPane } from "./ui/EditorPane";
import { DiffPane } from "./ui/DiffPane";
import { AuthPromptDialog } from "./ui/AuthPromptDialog";
import { KeysDialog } from "./ui/KeysDialog";
import { AboutDialog } from "./ui/AboutDialog";
import { CrashNotice } from "./ui/CrashNotice";
import { CommandPalette, type PaletteItem } from "./ui/CommandPalette";
import { Dialog } from "./ui/Dialog";
import { FilesPanel } from "./ui/FilesPanel";
import { HostKeyDialog } from "./ui/HostKeyDialog";
import { LocalNetworkDialog } from "./ui/LocalNetworkDialog";
import { RdpViewer } from "./ui/RdpViewer";
import { focusFirstItem, menuKeyDown } from "./ui/menuKeys";
import { ImportDialog } from "./ui/ImportDialog";
import { type PasswordAnswer, PasswordDialog } from "./ui/PasswordDialog";
import { SessionDialog } from "./ui/SessionDialog";
import { TunnelsPanel } from "./ui/TunnelsPanel";
import { FOLDER_REPORT_SETUP } from "./terminal/osc7";
import { MacroRecorder } from "./terminal/macro";
import { MacroSaveDialog } from "./ui/MacroSaveDialog";
import { ServerMonitor } from "./ui/ServerMonitor";
import { SnippetsPanel } from "./ui/SnippetsPanel";
import { NetworkTools } from "./ui/NetworkTools";
import { BroadcastDialog } from "./ui/BroadcastDialog";
import { type BroadcastState, eligibleTargets, resolveTargets } from "./ui/broadcast";
import { SerialQueue, chunkInput } from "./terminal/flow";
import { SettingsDialog } from "./ui/SettingsDialog";
import { UnlockDialog } from "./ui/MasterPassword";
import { SerialDialog } from "./ui/SerialDialog";
import { TransferSessionsDialog } from "./ui/TransferSessionsDialog";
import { type Tab, useTabs } from "./app/tabs";
import { useMasterPassword } from "./app/useMasterPassword";
import { useTheme } from "./app/useTheme";
import { SessionIconGlyph } from "./ui/sessionIcons";
import { isMacPlatform, matchShortcut } from "./ui/shortcuts";
import { Sidebar } from "./ui/Sidebar";
import { isFtp, targetLabel } from "./ui/sessionList";

const RECONNECT_DELAY_MS = 5000;

type SessionDialogState =
  | { mode: "create"; folder?: string }
  | { mode: "edit"; session: SessionRecord }
  | { mode: "quick" };

interface PasswordRequest {
  target: string;
  kind: "password" | "passphrase";
  canSave: boolean;
  retry: "wrong" | "savedRejected" | null;
  resolve: (answer: PasswordAnswer | null) => void;
}

const cancelled = () => ({ code: "cancelled", message: "Connection cancelled" });

export function App() {
  const [info, setInfo] = useState<AppInfo | null>(null);
  const { unlockOpen, setUnlockOpen, masterProtected, refreshMaster } = useMasterPassword();
  // Portable mode saves passwords only under a master password (ADR-033).
  const canSave = (info?.secretsAvailable ?? false) && (!info?.portable || masterProtected);
  const [sessions, setSessions] = useState<SessionRecord[]>([]);
  const { tabs, setTabs, tabsRef, active, setActive, addTab, renameTab, patchTab, splitTab, closeSplit, closeTab } =
    useTabs();
  const [sessionDialog, setSessionDialog] = useState<SessionDialogState | null>(null);
  // Several connections may ask at once (e.g. "Open all"): one dialog at a time.
  const [passwordRequests, setPasswordRequests] = useState<PasswordRequest[]>([]);
  const passwordRequest = passwordRequests[0] ?? null;
  const [hostKeyPrompts, setHostKeyPrompts] = useState<HostKeyPrompt[]>([]);
  const [authPrompts, setAuthPrompts] = useState<AuthPrompt[]>([]);
  const [confirmDelete, setConfirmDelete] = useState<SessionRecord | null>(null);
  const [confirmDiscard, setConfirmDiscard] = useState<Tab | null>(null);
  const [keysOpen, setKeysOpen] = useState(false);
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [aboutOpen, setAboutOpen] = useState(false);
  const [crashes, setCrashes] = useState<CrashReport[]>([]);
  const [tabMenu, setTabMenu] = useState<{ key: number; x: number; y: number } | null>(null);
  const tabMenuRef = useRef<HTMLDivElement>(null);
  /** Focus returns here when the tab menu closes from the keyboard. */
  const tabMenuOpener = useRef<HTMLElement | null>(null);
  useEffect(() => {
    if (tabMenu) focusFirstItem(tabMenuRef.current);
  }, [tabMenu]);
  const closeTabMenu = () => {
    setTabMenu(null);
    tabMenuOpener.current?.focus();
  };
  /** Arrow keys switch tabs (focus follows), Enter goes into the tab, Delete closes, F2 renames, Shift+F10 opens the menu. */
  const tabKey = (e: React.KeyboardEvent<HTMLButtonElement>, t: Tab) => {
    const i = tabs.findIndex((x) => x.key === t.key);
    const to = { ArrowRight: i + 1, ArrowLeft: i - 1, Home: 0, End: tabs.length - 1 }[e.key];
    if (to !== undefined) {
      e.preventDefault();
      const next = tabs[(to + tabs.length) % tabs.length];
      if (next) {
        setActive(next.key);
        document.getElementById(`tab-${next.key}`)?.focus();
      }
    } else if (e.key === "Enter" && !e.repeat) {
      // Into the tab's content (the terminal, the file list, …).
      e.preventDefault();
      document
        .getElementById(`pane-${t.key}`)
        ?.querySelector<HTMLElement>("textarea, input, select, button:not(:disabled)")
        ?.focus();
    } else if (e.key === "Delete") {
      e.preventDefault();
      requestClose(t.key);
      // Keep keyboard users in the tab bar (unless a confirmation opened).
      setTimeout(() => {
        if (document.activeElement === document.body)
          document.querySelector<HTMLElement>('[role="tab"][aria-selected="true"]')?.focus();
      }, 50);
    } else if (e.key === "F2") {
      e.preventDefault();
      setRenaming(t.key);
    } else if (e.key === "ContextMenu" || (e.key === "F10" && e.shiftKey)) {
      e.preventDefault();
      tabMenuOpener.current = e.currentTarget;
      const r = e.currentTarget.getBoundingClientRect();
      setTabMenu({ key: t.key, x: r.left, y: r.bottom });
    }
  };
  const [renaming, setRenaming] = useState<number | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  /** macOS blocked a local connection; shown once until dismissed. */
  const [localNetworkHelp, setLocalNetworkHelp] = useState(false);
  const localNetworkDismissed = useRef(false);
  useEffect(
    () =>
      onLocalNetworkBlocked(() => {
        if (!localNetworkDismissed.current) setLocalNetworkHelp(true);
      }),
    [],
  );
  const [dropped, setDropped] = useState<DroppedFile[] | null>(null);
  /** A folder dropped while the Network tools tab is active (for the file servers). */
  const [toolsFolder, setToolsFolder] = useState<DroppedFile | null>(null);
  /** Files are being dragged over the window. */
  const [fileHover, setFileHover] = useState(false);
  const [confirmForget, setConfirmForget] = useState<Tab | null>(null);
  const [importing, setImporting] = useState(false);
  const [settings, setSettings] = useState<Settings>(DEFAULT_SETTINGS);
  const didCheckUpdates = useRef(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [serialOpen, setSerialOpen] = useState(false);
  const [broadcast, setBroadcast] = useState<BroadcastState | null>(null);
  const [broadcastPicker, setBroadcastPicker] = useState<number | null>(null);
  const broadcastQueues = useRef(new Map<number, SerialQueue>());
  const [transferMode, setTransferMode] = useState<"export" | "import" | null>(null);
  const theme = useTheme(settings.theme);
  const activeRef = useRef<Tab | undefined>(undefined);

  const [folders, setFolders] = useState<string[]>([]);
  const [folderIcons, setFolderIcons] = useState<Map<string, SessionIcon>>(new Map());
  const refresh = useCallback(() => {
    listSessions().then(setSessions, (e) => setNotice(`Could not load sessions: ${String(e?.message ?? e)}`));
    listFolders().then(setFolders, () => {});
    listFolderIcons().then((list) => setFolderIcons(new Map(list.map(([p, i]) => [p.toLowerCase(), i]))), () => {});
  }, []);
  const failed = useCallback((e: unknown) => setNotice(String((e as { message?: string })?.message ?? e)), []);

  /** Closes a tab, asking first if an editor has unsaved changes. */
  const requestClose = useCallback(
    (key: number) => {
      const t = tabsRef.current.find((x) => x.key === key);
      if (t?.dirty) setConfirmDiscard(t);
      else closeTab(key);
    },
    [closeTab],
  );

  const openEditor = useCallback(
    (terminalId: TerminalId, dir: string, name: string) => {
      const existing = tabsRef.current.find(
        (t) => t.edit && t.edit.terminalId === terminalId && t.edit.dir === dir && t.edit.name === name,
      );
      if (existing) {
        setActive(existing.key);
        return;
      }
      const key = addTab(name, "editor", openLocalTerminal);
      patchTab(key, { edit: { terminalId, dir, name } });
    },
    [addTab, patchTab],
  );

  // Read through a ref so shortcut handlers keep a stable openLocal.
  const localPrefs = useRef<{ shells: ShellChoice[]; defaultShell: string | null }>({ shells: [], defaultShell: null });
  const [shells, setShells] = useState<ShellChoice[]>([]);
  localPrefs.current = { shells, defaultShell: settings.defaultShell };

  /** Opens a local terminal with `shellId`, else the preferred shell. */
  const openDiff = useCallback(
    (terminalId: TerminalId, a: { dir: string; name: string }, b: { dir: string; name: string }) => {
      const key = addTab(`${a.name} ↔ ${b.name}`, "diff", openLocalTerminal);
      patchTab(key, { diff: { left: { terminalId, ...a }, right: { terminalId, ...b } } });
    },
    [addTab, patchTab],
  );

  const openLocal = useCallback(
    (shellId?: string) => {
      const { shells: list, defaultShell } = localPrefs.current;
      const id = shellId ?? (list.some((s) => s.id === defaultShell) ? defaultShell : null);
      // No choice: the system default, which the backend lists first.
      const name = (id ? list.find((s) => s.id === id) : list[0])?.name ?? "Local";
      const key = addTab(name, "local", localTerminal(id));
      patchTab(key, { shellId: id });
      return key;
    },
    [addTab, patchTab],
  );

  const askPassword = useCallback(
    (req: Omit<PasswordRequest, "resolve">) =>
      new Promise<PasswordAnswer | null>((resolve) => setPasswordRequests((q) => [...q, { ...req, resolve }])),
    [],
  );

  /**
   * Runs a login (`attempt`), asking for a password or key passphrase when
   * the backend needs one and retrying; saving happens only after success.
   */
  const withLogin = useCallback(
    async <T extends { saveError?: string | null },>(
      label: string,
      opts: { canSave: boolean; keyAuth: boolean; savedSecret: boolean },
      attempt: (password: string | undefined, save: boolean) => Promise<T>,
    ): Promise<T> => {
      let secret: string | undefined;
      let save = false;
      let retry = false;
      for (;;) {
        try {
          const r = await attempt(secret, save);
          if (save) refresh();
          if (r.saveError) setNotice(`Connected, but the password was not saved: ${r.saveError}`);
          return r;
        } catch (e) {
          if (!isIpcError(e)) throw e;
          const needsPassword =
            e.code === "password_required" || (e.code === "auth_failed" && secret !== undefined && !opts.keyAuth);
          const needsPassphrase = opts.keyAuth && e.code === "key_error";
          if (!needsPassword && !needsPassphrase) throw e;
          const answer = await askPassword({
            target: label,
            kind: needsPassphrase ? "passphrase" : "password",
            canSave: opts.canSave,
            retry: retry ? "wrong" : e.code === "password_required" && opts.savedSecret ? "savedRejected" : null,
          });
          if (!answer) throw cancelled();
          secret = answer.password;
          save = answer.save;
          retry = true;
        }
      }
    },
    [askPassword, refresh],
  );

  const connect = useCallback(
    (
      target: SshTarget,
      label: string,
      opts: { canSave: boolean; keyAuth: boolean; savedSecret: boolean; startup?: string | null },
    ) => {
      let tabKey = 0;
      const open: Opener = async (cols, rows, onOutput, onExit) => {
        const r = await withLogin(label, opts, (password, savePassword) =>
          openSsh({ ...target, password, savePassword }, cols, rows, onOutput, onExit),
        );
        renameTab(tabKey, r.label);
        return r.id;
      };
      tabKey = addTab(label, "ssh", open);
      patchTab(tabKey, {
        reconnect: () => connect(target, label, opts),
        startup: opts.startup ?? undefined,
        sessionId: "sessionId" in target ? target.sessionId : undefined,
      });
    },
    [addTab, withLogin, renameTab, patchTab],
  );

  /** A file browser tab on a saved SSH session (no shell). */
  const openFilesTab = useCallback(
    (s: SessionRecord) => {
      const ftp = isFtp(s);
      const key = addTab(`${s.name} (files)`, "files", openLocalTerminal);
      patchTab(key, { sessionId: s.id, noShell: ftp });
      withLogin(
        s.name,
        { canSave, keyAuth: s.auth.kind === "keyFile", savedSecret: s.hasSavedSecret },
        (password, savePassword) =>
          (ftp ? openFtpFiles : openSshFiles)({ sessionId: s.id, password, savePassword }),
      ).then(
        (r) => patchTab(key, { terminalId: r.id }),
        (e) => patchTab(key, { ended: true, desktopError: isIpcError(e) ? e.message : String(e) }),
      );
    },
    [addTab, canSave, patchTab, withLogin],
  );

  /** Opens an RDP/VNC session in its external client, in tab `key`. */
  const runDesktop = useCallback(
    async (key: number, s: SessionRecord) => {
      patchTab(key, { desktop: null, desktopError: null });
      let password: string | undefined;
      for (;;) {
        try {
          const d = await openDesktop(s.id, password);
          password = undefined;
          patchTab(key, { desktop: d });
          return;
        } catch (e) {
          if (isIpcError(e) && e.code === "password_required") {
            const answer = await askPassword({ target: s.name, kind: "password", canSave: false, retry: null });
            if (answer) {
              password = answer.password;
              continue;
            }
            patchTab(key, { desktopError: "Cancelled." });
            return;
          }
          patchTab(key, { desktopError: isIpcError(e) ? e.message : String(e) });
          return;
        }
      }
    },
    [askPassword, patchTab],
  );

  const connectSaved = useCallback(
    (s: SessionRecord) => {
      if (s.protocol === "telnet") {
        const key = addTab(s.name, "telnet", telnetTerminal(s.id));
        patchTab(key, { sessionId: s.id, reconnect: () => connectSaved(s) });
        return;
      }
      if (s.protocol === "rdp" || s.protocol === "vnc") {
        const protocol = s.protocol;
        const embedded = () => {
          const key = addTab(s.name, "rdp", openLocalTerminal);
          patchTab(key, { sessionId: s.id });
        };
        if (settings.embeddedRdp) {
          embedded();
          return;
        }
        // Without an installed client the built-in viewer opens instead of
        // an error: it works on every system and needs nothing installed.
        void desktopClients()
          .then((c) => (protocol === "rdp" ? c.rdp : c.vnc))
          .catch(() => undefined)
          .then((client) => {
            if (client === null) {
              setNotice(
                `No ${protocol.toUpperCase()} client is installed, so ${s.name} opens inside Termalith. ` +
                  "Settings → Show RDP and VNC sessions inside Termalith makes this the default.",
              );
              embedded();
              return;
            }
            const key = addTab(s.name, "desktop", openLocalTerminal);
            patchTab(key, { sessionId: s.id });
            void runDesktop(key, s);
          });
        return;
      }
      if (s.filesOnly || isFtp(s)) {
        openFilesTab(s);
        return;
      }
      if (s.mosh) {
        const key = addTab(s.name, "local", moshTerminal(s.id));
        patchTab(key, { sessionId: s.id, startup: s.startupCommand ?? undefined });
        return;
      }
      connect({ sessionId: s.id }, s.name, {
        canSave,
        keyAuth: s.auth.kind === "keyFile",
        savedSecret: s.hasSavedSecret,
        startup: s.startupCommand,
      });
    },
    [addTab, canSave, connect, openFilesTab, patchTab, runDesktop, settings.embeddedRdp],
  );

  // Remote desktop status: poll while a client is running.
  const watchDesktops = tabs.some((t) => t.desktop?.kind === "running");
  useEffect(() => {
    if (!watchDesktops) return;
    const timer = window.setInterval(() => {
      listDesktops().then(
        (list) =>
          setTabs((ts) =>
            ts.map((t) => {
              const d = t.desktop && list.find((x) => x.id === t.desktop?.id);
              return d && d.kind !== t.desktop?.kind ? { ...t, desktop: d } : t;
            }),
          ),
        () => {},
      );
    }, 2000);
    return () => window.clearInterval(timer);
  }, [watchDesktops]);

  // Closing a file browser tab releases its SSH connection.
  const openFileIds = useRef(new Set<number>());
  useEffect(() => {
    const now = new Set(tabs.flatMap((t) => (t.kind === "files" && t.terminalId !== undefined ? [t.terminalId] : [])));
    for (const id of openFileIds.current) if (!now.has(id)) void closeTerminal(id).catch(() => {});
    openFileIds.current = now;
  }, [tabs]);

  // Closing a desktop tab (by any route) stops its client and tunnel.
  const openDesktopIds = useRef(new Set<number>());
  useEffect(() => {
    const now = new Set(tabs.flatMap((t) => (t.desktop ? [t.desktop.id] : [])));
    for (const id of openDesktopIds.current) if (!now.has(id)) void closeDesktop(id).catch(() => {});
    openDesktopIds.current = now;
  }, [tabs]);

  const didOpenInitialTab = useRef(false);
  useEffect(() => {
    appInfo().then(setInfo, () => setInfo({ version: "?", secretsAvailable: false, agentAvailable: false }));
    crashReports().then(setCrashes, () => {});
    refresh();
    // The first tab uses the preferred shell, so wait for settings and the
    // shell list. StrictMode runs effects twice in development; open once.
    void Promise.all([
      getSettings().then(
        (s) => {
          setSettings(s);
          localPrefs.current.defaultShell = s.defaultShell;
          // Opt-in: one quiet request; only a newer version is mentioned.
          if (s.checkUpdates && !didCheckUpdates.current) {
            didCheckUpdates.current = true;
            checkForUpdates().then(
              (u) => u.newer && setNotice(`Termalith ${u.latest} is available. Open About (ⓘ) to download it.`),
              () => {},
            );
          }
        },
        () => {},
      ),
      listShells().then(
        (list) => {
          setShells(list);
          localPrefs.current.shells = list;
        },
        () => {},
      ),
    ]).then(() => {
      if (!didOpenInitialTab.current) {
        didOpenInitialTab.current = true;
        openLocal();
      }
    });
    const unlisten = listen<HostKeyPrompt>("host-key-prompt", (e) =>
      setHostKeyPrompts((q) => [...q, e.payload]),
    );
    const unlistenAuth = listen<AuthPrompt>("auth-prompt", (e) => setAuthPrompts((q) => [...q, e.payload]));
    const unlistenDrop = listen<DroppedFile[]>("files-dropped", (e) => {
      const tab = activeRef.current;
      if (tab?.kind === "tools") {
        const folder = e.payload.find((f) => f.folder);
        if (folder) setToolsFolder(folder);
        else setNotice("Drop a folder to share it with the file servers.");
        return;
      }
      if (!(tab?.filesOpen || tab?.kind === "files") || tab.terminalId === undefined)
        setNotice("To upload, open Files on a connected SSH tab and drop the files again.");
      else if (e.payload.length === 0)
        setNotice("Nothing to upload: only files and folders can be dropped (a folder may be too large to read).");
      else setDropped(e.payload);
    });
    const unlistenHover = listen<boolean>("files-hover", (e) => setFileHover(e.payload));
    return () => {
      void unlisten.then((f) => f());
      void unlistenDrop.then((f) => f());
      void unlistenHover.then((f) => f());
      void unlistenAuth.then((f) => f());
    };
  }, [refresh, openLocal]);

  async function submitSession(value: SessionInput) {
    const d = sessionDialog;
    if (!d) return;
    if (d.mode === "quick") {
      setSessionDialog(null);
      connect({ quick: value }, targetLabel(value), {
        canSave: false,
        keyAuth: value.auth.kind === "keyFile",
        savedSecret: false,
      });
      return;
    }
    if (d.mode === "edit") await updateSession(d.session.id, value);
    else await createSession(value);
    setSessionDialog(null);
    refresh();
  }

  function answerHostKey(prompt: HostKeyPrompt, answer: HostKeyAnswer) {
    setHostKeyPrompts((q) => q.filter((p) => p.requestId !== prompt.requestId));
    void respondHostKey(prompt.requestId, answer);
  }

  // Keyboard shortcuts (captured before the terminal sees the key).
  const shortcutState = useRef({ tabs, active, settings });
  shortcutState.current = { tabs, active, settings };
  useEffect(() => {
    const mac = isMacPlatform();
    const onKey = (e: KeyboardEvent) => {
      const action = matchShortcut(e, mac);
      if (!action) return;
      if (document.querySelector("dialog[open]")) return; // dialogs keep their keys
      e.preventDefault();
      e.stopPropagation();
      const { tabs: ts, active: a, settings: s } = shortcutState.current;
      const i = ts.findIndex((t) => t.key === a);
      switch (action) {
        case "newTab":
          openLocal();
          break;
        case "closeTab":
          if (a !== null) requestClose(a);
          break;
        case "nextTab":
        case "prevTab":
          if (ts.length) {
            const n = (i + (action === "nextTab" ? 1 : -1) + ts.length) % ts.length;
            setActive(ts[n]?.key ?? null);
          }
          break;
        case "zoomIn":
        case "zoomOut":
        case "zoomReset": {
          const size = action === "zoomReset" ? 13 : Math.min(32, Math.max(8, s.fontSize + (action === "zoomIn" ? 1 : -1)));
          const next = { ...s, fontSize: size };
          setSettings(next);
          void saveSettings(next).catch(() => {});
          break;
        }
        case "palette":
          setPaletteOpen(true);
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
          // Like browsers: 9 is the last tab.
          const n = Number(action.slice(3));
          const t = n === 9 ? ts[ts.length - 1] : ts[n - 1];
          if (t) setActive(t.key);
          break;
        }
        default:
          if (a !== null) patchTab(a, { command: { seq: performance.now(), action } });
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [openLocal, requestClose, patchTab]);

  // Automatic reconnect of dropped SSH tabs, with a visible countdown.
  const [now, setNow] = useState(() => Date.now());
  const pendingReconnect = tabs.some((t) => t.reconnectAt !== undefined);
  useEffect(() => {
    if (!pendingReconnect) return;
    const timer = window.setInterval(() => setNow(Date.now()), 500);
    return () => window.clearInterval(timer);
  }, [pendingReconnect]);
  useEffect(() => {
    for (const t of tabs) {
      if (t.reconnectAt !== undefined && now >= t.reconnectAt && t.reconnect) {
        const again = t.reconnect;
        closeTab(t.key);
        again();
      }
    }
  }, [now, tabs, closeTab]);

  useEffect(() => {
    if (!tabMenu) return;
    const close = (e: MouseEvent) => {
      if (!(e.target as Element).closest?.(".tab-menu")) setTabMenu(null);
    };
    const esc = (e: KeyboardEvent) => e.key === "Escape" && setTabMenu(null);
    window.addEventListener("mousedown", close);
    window.addEventListener("keydown", esc);
    return () => {
      window.removeEventListener("mousedown", close);
      window.removeEventListener("keydown", esc);
    };
  }, [tabMenu]);

  /** A new tab like `t`: another local shell, or a new connection to the same session. */
  function duplicateTab(t: Tab) {
    if (t.kind === "local") openLocal(t.shellId ?? undefined);
    else if (t.kind === "ssh" && t.reconnect) t.reconnect();
  }

  function closeOthers(keep: number) {
    const dirty = tabs.filter((t) => t.key !== keep && t.dirty);
    for (const t of tabs) if (t.key !== keep && !t.dirty) closeTab(t.key);
    setActive(keep);
    if (dirty.length) setNotice(`Kept ${dirty.map((t) => t.title).join(", ")}: unsaved changes.`);
  }

  // Stop broadcasting when the source tab closes or its session ends.
  useEffect(() => {
    if (!broadcast) return;
    const src = tabs.find((t) => t.key === broadcast.source);
    if (!src || src.ended) setBroadcast(null);
  }, [tabs, broadcast]);

  const [recording, setRecording] = useState<{ key: number; recorder: MacroRecorder } | null>(null);
  const [macroToSave, setMacroToSave] = useState<MacroStep[] | null>(null);
  const [macrosVersion, setMacrosVersion] = useState(0);

  // A closed tab ends its recording; nothing typed there is kept.
  useEffect(() => {
    if (recording && !tabs.some((t) => t.key === recording.key)) setRecording(null);
  }, [recording, tabs]);

  /** A session's own font and colors over the settings; the same object while nothing changes. */
  const lookCache = useRef(new Map<string, Settings>());
  const settingsFor = (sessionId: string | undefined): Settings => {
    const look = sessionId ? sessions.find((x) => x.id === sessionId)?.appearance : null;
    if (!look) return settings;
    const key = JSON.stringify([look, settings]);
    let merged = lookCache.current.get(key);
    if (!merged) {
      merged = {
        ...settings,
        fontFamily: look.fontFamily ?? settings.fontFamily,
        fontSize: look.fontSize ?? settings.fontSize,
        terminalScheme: look.terminalScheme ?? settings.terminalScheme,
      };
      if (lookCache.current.size > 50) lookCache.current.clear();
      lookCache.current.set(key, merged);
    }
    return merged;
  };

  const mirrorInput = (sourceKey: number, data: string) => {
    if (recording?.key === sourceKey) recording.recorder.add(data);
    if (!broadcast || broadcast.source !== sourceKey) return;
    for (const { key, terminalId } of resolveTargets(broadcast, tabs).live) {
      let q = broadcastQueues.current.get(key);
      if (!q) {
        q = new SerialQueue(() => {});
        broadcastQueues.current.set(key, q);
      }
      for (const chunk of chunkInput(data)) q.push(() => writeTerminal(terminalId, chunk));
    }
  };

  async function toggleLog(tab: Tab) {
    if (tab.terminalId === undefined) return;
    try {
      if (tab.log) {
        await stopLog(tab.terminalId);
        patchTab(tab.key, { log: null });
        setNotice(`Session log saved: ${tab.log.path}`);
      } else {
        const log = await startLog(tab.terminalId, tab.title);
        patchTab(tab.key, { log });
      }
    } catch (e) {
      setNotice(`Session log: ${isIpcError(e) ? e.message : String(e)}`);
    }
  }

  const currentPrompt = hostKeyPrompts[0];
  const activeTab = tabs.find((t) => t.key === active);
  activeRef.current = activeTab;

  function onTabFailed(tab: Tab, e: unknown) {
    if (!isIpcError(e) || e.code !== "host_key_changed") return;
    const d = (e as IpcError).detail as Tab["changedKey"] | undefined;
    if (d && typeof d.host === "string") patchTab(tab.key, { changedKey: d });
  }

  async function forget(tab: Tab) {
    setConfirmForget(null);
    if (!tab.changedKey) return;
    try {
      const r = await forgetHostKey(tab.changedKey.host, tab.changedKey.port);
      patchTab(tab.key, { changedKey: undefined });
      setNotice(
        r.remainingInUserFile > 0
          ? `Removed ${r.removed} key(s) from Termalith. The old key is also in your OpenSSH known_hosts; remove it there with: ssh-keygen -R ${tab.changedKey.host}`
          : `Removed ${r.removed} recorded key(s). Reconnect to review the new key.`,
      );
    } catch (err) {
      setNotice(`Could not remove the key: ${String((err as IpcError)?.message ?? err)}`);
    }
  }

  return (
    <div className="app">
      <Sidebar
        sessions={sessions}
        folders={folders}
        folderIcons={folderIcons}
        onSetFolderIcon={(path, icon) => void setFolderIcon(path, icon).then(refresh, failed)}
        onConnect={connectSaved}
        onOpenFiles={openFilesTab}
        onEdit={(s) => setSessionDialog({ mode: "edit", session: s })}
        onDelete={setConfirmDelete}
        onForgetSecret={(s) =>
          forgetSessionSecret(s.id).then(refresh, (e) => setNotice(String(e?.message ?? e)))
        }
        onNew={(folder) => setSessionDialog({ mode: "create", folder })}
        onCreateFolder={(path) => createFolder(path).then(refresh)}
        onRenameFolder={(from, to) =>
          renameFolder(from, to).then(refresh, (e) => {
            failed(e);
            throw e;
          })
        }
        onDeleteFolder={(path) => deleteFolder(path).then(refresh, failed)}
        onMoveSession={(s, folder) => moveSession(s.id, folder).then(refresh, failed)}
        onQuickConnect={() => setSessionDialog({ mode: "quick" })}
        shells={shells}
        onLocalTerminal={(id) => openLocal(id)}
        onImport={() => setImporting(true)}
        onToggleFavorite={(s) => setFavorite(s.id, !s.favorite).then(refresh, (e) => setNotice(String(e?.message ?? e)))}
        onDuplicate={(s) => {
          const { id: _id, hasSavedSecret: _h, favorite: _f, updatedAt: _u, ...input } = s;
          createSession({ ...input, name: `${s.name} (copy)` }).then(refresh, (e) => setNotice(String(e?.message ?? e)));
        }}
        onExport={() => setTransferMode("export")}
        onImportSessions={() => setTransferMode("import")}
        onSettings={() => setSettingsOpen(true)}
        onSerial={() => setSerialOpen(true)}
        onKeys={() => setKeysOpen(true)}
        onAbout={() => setAboutOpen(true)}
        onTools={() => {
          const existing = tabs.find((t) => t.kind === "tools");
          if (existing) setActive(existing.key);
          else addTab("Network tools", "tools", openLocalTerminal);
        }}
      />
      <main className="workspace">
        <div className="tabbar" role="tablist" aria-label="Open tabs">
          {tabs.map((t) => (
            <div
              key={t.key}
              className={`tab ${t.key === active ? "active" : ""} ${broadcast && (broadcast.source === t.key || broadcast.targets.includes(t.key)) ? "broadcasting" : ""}`}
              role="presentation"
              onContextMenu={(e) => {
                e.preventDefault();
                setTabMenu({ key: t.key, x: e.clientX, y: e.clientY });
              }}
            >
              {renaming === t.key ? (
                <input
                  className="tab-rename"
                  autoFocus
                  defaultValue={t.title}
                  aria-label="Tab name"
                  onFocus={(e) => e.target.select()}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") e.currentTarget.blur();
                    if (e.key === "Escape") {
                      e.currentTarget.value = t.title;
                      e.currentTarget.blur();
                    }
                  }}
                  onBlur={(e) => {
                    const name = e.target.value.trim().slice(0, 80);
                    if (name) renameTab(t.key, name);
                    setRenaming(null);
                  }}
                />
              ) : (
                <button
                  type="button"
                  role="tab"
                  id={`tab-${t.key}`}
                  aria-selected={t.key === active}
                  aria-controls={`pane-${t.key}`}
                  tabIndex={t.key === active ? 0 : -1}
                  className="tab-title"
                  onClick={() => setActive(t.key)}
                  onDoubleClick={() => setRenaming(t.key)}
                  onKeyDown={(e) => tabKey(e, t)}
                >
                  {(() => {
                    const icon = t.sessionId ? sessions.find((x) => x.id === t.sessionId)?.icon : null;
                    return icon ? (
                      <SessionIconGlyph icon={icon} size={13} className="tab-icon" />
                    ) : (
                      <span className={`tab-kind ${t.kind}`} aria-hidden="true" />
                    );
                  })()}
                  {t.title}
                </button>
              )}
              {/* Not in the Tab order: Delete closes the focused tab. */}
              <button
                type="button"
                className="tab-close"
                tabIndex={-1}
                aria-label={`Close ${t.title}`}
                onClick={() => requestClose(t.key)}
              >
                ×
              </button>
            </div>
          ))}
          {tabMenu && tabs.find((t) => t.key === tabMenu.key) && (() => {
            const t = tabs.find((x) => x.key === tabMenu.key) as Tab;
            const canDuplicate = t.kind === "local" || (t.kind === "ssh" && !!t.reconnect);
            const act = (f: () => void) => () => {
              closeTabMenu();
              f();
            };
            return (
              <div
                ref={tabMenuRef}
                className="menu tab-menu"
                role="menu"
                style={{ position: "fixed", left: tabMenu.x, top: tabMenu.y }}
                onKeyDown={(e) => menuKeyDown(e, closeTabMenu)}
              >
                <button type="button" role="menuitem" onClick={act(() => setRenaming(t.key))}>
                  Rename…
                </button>
                {canDuplicate && (
                  <button type="button" role="menuitem" onClick={act(() => duplicateTab(t))}>
                    Duplicate
                  </button>
                )}
                {t.terminalId !== undefined && ["local", "ssh", "serial", "telnet"].includes(t.kind) && (
                  <button
                    type="button"
                    role="menuitem"
                    onClick={act(() => patchTab(t.key, { command: { seq: performance.now(), action: "save" } }))}
                  >
                    Save output…
                  </button>
                )}
                <button type="button" role="menuitem" disabled={tabs.length < 2} onClick={act(() => closeOthers(t.key))}>
                  Close others
                </button>
                <button type="button" role="menuitem" onClick={act(() => requestClose(t.key))}>
                  Close
                </button>
              </div>
            );
          })()}
          <button type="button" className="tab-new" aria-label="New local terminal" title="New local terminal" onClick={() => openLocal()}>
            +
          </button>
        </div>
        {notice && (
          <div className="notice" role="status">
            {notice}
            <button type="button" className="icon" aria-label="Dismiss" onClick={() => setNotice(null)}>
              ×
            </button>
          </div>
        )}
        <div className="panes">
          {tabs.length === 0 && (
            <div className="placeholder">
              <p>No open terminals.</p>
              <p>Connect to a saved session, use Quick connect, or open a local terminal.</p>
            </div>
          )}
          {tabs.map((t) => (
            <div
              key={t.key}
              id={`pane-${t.key}`}
              className="pane"
              hidden={t.key !== active}
              role="tabpanel"
              aria-labelledby={`tab-${t.key}`}
            >
              {t.kind === "tools" ? (
                <NetworkTools dropped={toolsFolder} />
              ) : t.kind === "diff" && t.diff ? (
                <DiffPane left={t.diff.left} right={t.diff.right} />
              ) : t.kind === "editor" && t.edit ? (
                <EditorPane
                  terminalId={t.edit.terminalId}
                  dir={t.edit.dir}
                  name={t.edit.name}
                  active={t.key === active}
                  onDirty={(dirty) => {
                    if (dirty !== !!t.dirty) patchTab(t.key, { dirty });
                  }}
                />
              ) : t.kind === "files" ? (
                <div className="files-tab">
                  {t.terminalId !== undefined ? (
                    <>
                      <div className="pane-toolbar">
                        <span />
                        <span className="pane-actions">
                          {!t.noShell && (
                            <button
                              type="button"
                              title="Open a terminal on this connection (no second login)"
                              onClick={() => {
                                if (t.terminalId !== undefined)
                                  addTab(t.title.replace(/ \(files\)$/, ""), "ssh", splitTerminal(t.terminalId));
                              }}
                            >
                              Open terminal
                            </button>
                          )}
                        </span>
                      </div>
                      <FilesPanel
                        terminalId={t.terminalId}
                        dropped={t.key === active ? dropped : null}
                        fileHover={t.key === active && fileHover}
                        onDroppedHandled={() => setDropped(null)}
                        onClose={() => closeTab(t.key)}
                        onEdit={(dir, name) => t.terminalId !== undefined && openEditor(t.terminalId, dir, name)}
                        onCompare={(a, b) => t.terminalId !== undefined && openDiff(t.terminalId, a, b)}
                        connection={t.title.replace(/ \(files\)$/, "")}
                      />
                    </>
                  ) : (
                    <div className="desktop-pane">
                      <div className="desktop-card">
                        {t.desktopError ? (
                          <p className="form-error" role="alert">
                            {t.desktopError}
                          </p>
                        ) : (
                          <p>Connecting…</p>
                        )}
                      </div>
                    </div>
                  )}
                </div>
              ) : t.kind === "rdp" && t.sessionId ? (
                <RdpViewer
                  sessionId={t.sessionId}
                  name={t.title}
                  keyboard={sessions.find((x) => x.id === t.sessionId)?.protocol === "vnc" ? "keysym" : "scancode"}
                  askPassword={(retry) =>
                    askPassword({
                      target: t.title,
                      kind: "password",
                      canSave,
                      retry: retry ?? (sessions.find((x) => x.id === t.sessionId)?.hasSavedSecret ? "savedRejected" : null),
                    })
                  }
                  onPasswordSaved={(error) => {
                    refresh();
                    if (error) setNotice(`Connected, but the password was not saved: ${error}`);
                  }}
                  onClose={() => closeTab(t.key)}
                />
              ) : t.kind === "desktop" ? (
                <DesktopPane
                  info={t.desktop ?? null}
                  error={t.desktopError ?? null}
                  onClose={() => closeTab(t.key)}
                  onReopen={() => {
                    // Replacing `desktop` closes the previous client (see the effect above).
                    const s = sessions.find((x) => x.id === t.sessionId);
                    if (s) void runDesktop(t.key, s);
                    else patchTab(t.key, { desktopError: "The saved session no longer exists." });
                  }}
                />
              ) : (
              <>
              <div className="pane-toolbar">
                  {t.reconnectAt !== undefined ? (
                    <span className="pane-warning" role="status">
                      Connection lost. Reconnecting in {Math.max(0, Math.ceil((t.reconnectAt - now) / 1000))} s…
                      <button type="button" onClick={() => patchTab(t.key, { reconnectAt: undefined })}>
                        Cancel
                      </button>
                    </span>
                  ) : t.changedKey ? (
                    <span className="pane-warning">
                      Host key changed for {t.changedKey.host}.
                      <button type="button" className="danger" onClick={() => setConfirmForget(t)}>
                        Remove recorded key…
                      </button>
                    </span>
                  ) : (
                    <span />
                  )}
                  <span className="pane-actions">
                    <button
                      type="button"
                      title="Split side by side"
                      aria-label="Split side by side"
                      disabled={t.terminalId === undefined || t.ended || t.kind === "serial" || t.kind === "telnet"}
                      onClick={() => splitTab(t, "row")}
                    >
                      ◫
                    </button>
                    <button
                      type="button"
                      title="Split top and bottom"
                      aria-label="Split top and bottom"
                      disabled={t.terminalId === undefined || t.ended || t.kind === "serial" || t.kind === "telnet"}
                      onClick={() => splitTab(t, "column")}
                    >
                      ⊟
                    </button>
                    <button
                      type="button"
                      title="Four panes"
                      aria-label="Four panes"
                      disabled={t.terminalId === undefined || t.ended || t.kind === "serial" || t.kind === "telnet"}
                      onClick={() => splitTab(t, "grid")}
                    >
                      ⊞
                    </button>
                    {broadcast?.source === t.key ? (
                      <button type="button" className="danger" onClick={() => setBroadcast(null)}>
                        Stop broadcast
                      </button>
                    ) : (
                      <button
                        type="button"
                        title="Type into several terminals at once"
                        disabled={t.terminalId === undefined || t.ended || broadcast !== null}
                        onClick={() => setBroadcastPicker(t.key)}
                      >
                        Broadcast…
                      </button>
                    )}
                    <button
                      type="button"
                      aria-pressed={!!t.log}
                      className={t.log ? "recording" : undefined}
                      title={t.log ? `Logging to ${t.log.path}. Click to stop.` : "Save this terminal's output to a text file"}
                      disabled={t.terminalId === undefined || t.ended}
                      onClick={() => void toggleLog(t)}
                    >
                      {t.log ? "● Log" : "Log"}
                    </button>
                    <button
                      type="button"
                      aria-pressed={recording?.key === t.key}
                      className={recording?.key === t.key ? "recording" : undefined}
                      title={recording?.key === t.key ? "Stop recording" : "Record what you type as a macro"}
                      disabled={t.terminalId === undefined || t.ended || (recording !== null && recording.key !== t.key)}
                      onClick={() => {
                        if (recording?.key === t.key) {
                          setMacroToSave(recording.recorder.steps);
                          setRecording(null);
                        } else {
                          setRecording({ key: t.key, recorder: new MacroRecorder() });
                        }
                      }}
                    >
                      {recording?.key === t.key ? "■ Macro" : "Macro"}
                    </button>
                    <button
                      type="button"
                      aria-pressed={!!t.snippetsOpen}
                      disabled={t.terminalId === undefined || t.ended}
                      onClick={() => patchTab(t.key, { snippetsOpen: !t.snippetsOpen, filesOpen: false, tunnelsOpen: false })}
                    >
                      Snippets
                    </button>
                    {t.reconnect && t.ended && (
                      <button type="button" onClick={() => { closeTab(t.key); t.reconnect?.(); }}>
                        Reconnect
                      </button>
                    )}
                    {t.kind === "ssh" && (
                      <>
                        <button
                          type="button"
                          aria-pressed={!!t.tunnelsOpen}
                          disabled={t.terminalId === undefined || t.ended}
                          onClick={() => patchTab(t.key, { tunnelsOpen: !t.tunnelsOpen, filesOpen: false, snippetsOpen: false })}
                        >
                          Tunnels
                        </button>
                        <button
                          type="button"
                          aria-pressed={!!t.filesOpen}
                          disabled={t.terminalId === undefined || t.ended}
                          onClick={() => patchTab(t.key, { filesOpen: !t.filesOpen, tunnelsOpen: false, snippetsOpen: false })}
                        >
                          Files
                        </button>
                      </>
                    )}
                  </span>
              </div>
              {broadcast?.source === t.key && (
                <div className="broadcast-banner" role="status">
                  BROADCASTING to {resolveTargets(broadcast, tabs).live.map((x) => tabs.find((y) => y.key === x.key)?.title).join(", ") || "no live terminals"}
                  {resolveTargets(broadcast, tabs).skipped.length > 0 &&
                    ` · skipped (not connected): ${resolveTargets(broadcast, tabs).skipped.map((x) => x.title).join(", ")}`}
                </div>
              )}
              {broadcast?.targets.includes(t.key) && broadcast.source !== t.key && (
                <div className="broadcast-banner receiving" role="status">
                  Receiving input broadcast from {tabs.find((y) => y.key === broadcast.source)?.title}
                </div>
              )}
              {recording?.key === t.key && (
                <div className="broadcast-banner" role="status">
                  RECORDING: everything you type here is captured, including passwords.
                </div>
              )}
              <div className="pane-body">
                <div className={`pane-terminals ${t.direction}`}>
                  <div className="pane-terminal">
                    <TerminalView
                      open={t.open}
                      active={t.key === active}
                      onReady={(id) => {
                        patchTab(t.key, { terminalId: id, startup: undefined });
                        if (t.startup) void writeTerminal(id, `${t.startup}\r`).catch(() => {});
                        if (settings.logSessions)
                          startLog(id, t.title).then(
                            (log) => patchTab(t.key, { log }),
                            (e) => setNotice(`Session log: ${isIpcError(e) ? e.message : String(e)}`),
                          );
                      }}
                      onFailed={(e) => {
                        patchTab(t.key, { ended: true });
                        onTabFailed(t, e);
                      }}
                      onExited={(payload) =>
                        patchTab(t.key, {
                          ended: true,
                          filesOpen: false,
                          tunnelsOpen: false,
                          snippetsOpen: false,
                          log: null,
                          // No exit status: the connection dropped, the shell did not exit.
                          reconnectAt:
                            t.kind === "ssh" && !payload.exit && t.reconnect && settings.autoReconnect
                              ? Date.now() + RECONNECT_DELAY_MS
                              : undefined,
                        })
                      }
                      settings={settingsFor(t.sessionId)}
                      theme={theme}
                      command={t.command}
                      label={t.title}
                      onNotice={setNotice}
                      onInput={(d) => mirrorInput(t.key, d)}
                      onFolder={(host, path) =>
                        setTabs((all) =>
                          all.map((x) =>
                            // Another host: a nested ssh, whose paths mean nothing here.
                            x.key === t.key && (!x.shellFolder || x.shellFolder.host === host)
                              ? { ...x, shellFolder: { host, path } }
                              : x,
                          ),
                        )
                      }
                    />
                  </div>
                  {t.splits.map((s) => (
                    <div key={s.key} className="pane-terminal split">
                      <button
                        type="button"
                        className="split-close icon"
                        aria-label="Close pane"
                        title="Close pane"
                        onClick={() => closeSplit(t.key, s.key)}
                      >
                        ×
                      </button>
                      <TerminalView open={s.open} active={t.key === active} settings={settingsFor(t.sessionId)} theme={theme} />
                    </div>
                  ))}
                </div>
                {t.snippetsOpen && t.terminalId !== undefined && (
                  <SnippetsPanel
                    terminalId={t.terminalId}
                    onClose={() => patchTab(t.key, { snippetsOpen: false })}
                    macrosVersion={macrosVersion}
                  />
                )}
                {t.tunnelsOpen && t.terminalId !== undefined && (
                  <TunnelsPanel terminalId={t.terminalId} onClose={() => patchTab(t.key, { tunnelsOpen: false })} />
                )}
                {t.filesOpen && t.terminalId !== undefined && (
                  <FilesPanel
                    terminalId={t.terminalId}
                    dropped={t.key === active ? dropped : null}
                    fileHover={t.key === active && fileHover}
                    onDroppedHandled={() => setDropped(null)}
                    onClose={() => patchTab(t.key, { filesOpen: false })}
                    onEdit={(dir, name) => t.terminalId !== undefined && openEditor(t.terminalId, dir, name)}
                    onCompare={(a, b) => t.terminalId !== undefined && openDiff(t.terminalId, a, b)}
                    connection={t.title}
                    follow={{
                      on: !!t.followFolder,
                      folder: t.shellFolder?.path,
                      onToggle: () => patchTab(t.key, { followFolder: !t.followFolder }),
                      onSetup: () => {
                        if (t.terminalId !== undefined)
                          void writeTerminal(t.terminalId, `${FOLDER_REPORT_SETUP}\r`).catch(() => {});
                      },
                    }}
                  />
                )}
              </div>
              {settings.serverMonitor && t.kind === "ssh" && t.terminalId !== undefined && !t.ended && (
                <ServerMonitor terminalId={t.terminalId} active={t.key === active} />
              )}
              </>
              )}
            </div>
          ))}
        </div>
      </main>

      {sessionDialog && (
        <SessionDialog
          mode={sessionDialog.mode}
          initial={sessionDialog.mode === "edit" ? sessionDialog.session : undefined}
          editingId={sessionDialog.mode === "edit" ? sessionDialog.session.id : undefined}
          defaultFolder={sessionDialog.mode === "create" ? sessionDialog.folder : undefined}
          sessions={sessions}
          folders={folders}
          onSubmit={submitSession}
          onCancel={() => setSessionDialog(null)}
        />
      )}
      {passwordRequest && (
        <PasswordDialog
          key={passwordRequests.length + passwordRequest.target}
          target={passwordRequest.target}
          kind={passwordRequest.kind}
          canSave={passwordRequest.canSave}
          retry={passwordRequest.retry}
          onSubmit={(a) => {
            setPasswordRequests((q) => q.slice(1));
            passwordRequest.resolve(a);
          }}
          onCancel={() => {
            setPasswordRequests((q) => q.slice(1));
            passwordRequest.resolve(null);
          }}
        />
      )}
      {!currentPrompt && authPrompts[0] && (
        <AuthPromptDialog
          key={authPrompts[0].requestId}
          prompt={authPrompts[0]}
          onAnswer={(answers) => {
            const p = authPrompts[0];
            if (!p) return;
            setAuthPrompts((q) => q.filter((x) => x.requestId !== p.requestId));
            void respondAuthPrompt(p.requestId, answers);
          }}
        />
      )}
      {localNetworkHelp && (
        <LocalNetworkDialog
          onClose={() => {
            // Later failures still say where to fix it in their message.
            localNetworkDismissed.current = true;
            setLocalNetworkHelp(false);
          }}
        />
      )}
      {currentPrompt && (
        <HostKeyDialog
          key={currentPrompt.requestId}
          prompt={currentPrompt}
          onAnswer={(a) => answerHostKey(currentPrompt, a)}
        />
      )}
      {unlockOpen && <UnlockDialog onClose={() => setUnlockOpen(false)} onReset={refresh} />}

      {macroToSave && (
        <MacroSaveDialog
          steps={macroToSave}
          onDiscard={() => setMacroToSave(null)}
          onSave={(name) => {
            createMacro({ name, steps: macroToSave }).then(
              () => {
                setMacroToSave(null);
                setMacrosVersion((v) => v + 1);
              },
              (e) => setNotice(`Macro: ${isIpcError(e) ? e.message : String(e)}`),
            );
          }}
        />
      )}

      {broadcastPicker !== null && tabs.find((t) => t.key === broadcastPicker) && (
        <BroadcastDialog
          source={tabs.find((t) => t.key === broadcastPicker)!}
          candidates={eligibleTargets(tabs, broadcastPicker)}
          onCancel={() => setBroadcastPicker(null)}
          onStart={(targets) => {
            broadcastQueues.current.clear();
            setBroadcast({ source: broadcastPicker, targets });
            setBroadcastPicker(null);
          }}
        />
      )}
      {serialOpen && (
        <SerialDialog
          onCancel={() => setSerialOpen(false)}
          onOpen={(config) => {
            setSerialOpen(false);
            addTab(`${config.path.split("/").pop()} ${serialSummary(config)}`, "serial", openSerial(config));
          }}
        />
      )}
      {settingsOpen && (
        <SettingsDialog
          initial={settings}
          shells={shells}
          secretsAvailable={info?.secretsAvailable ?? false}
          portable={info?.portable ?? false}
          onSecretsReset={refresh}
          onCancel={() => {
            setSettingsOpen(false);
            refreshMaster();
          }}
          onSave={async (next) => {
            await saveSettings(next);
            setSettings(next);
            setSettingsOpen(false);
          }}
        />
      )}
      {transferMode && (
        <TransferSessionsDialog
          mode={transferMode}
          onCancel={() => setTransferMode(null)}
          onDone={(summary) => {
            setTransferMode(null);
            setNotice(summary);
            refresh();
          }}
        />
      )}
      {importing && (
        <ImportDialog
          onCancel={() => setImporting(false)}
          onDone={(summary) => {
            setImporting(false);
            setNotice(summary);
            refresh();
          }}
        />
      )}
      {confirmForget?.changedKey && (
        <Dialog
          title="Remove the recorded host key?"
          tone="warning"
          onCancel={() => setConfirmForget(null)}
          actions={
            <>
              <button type="button" onClick={() => setConfirmForget(null)}>
                Cancel
              </button>
              <button type="button" className="danger" onClick={() => void forget(confirmForget)}>
                Remove key
              </button>
            </>
          }
        >
          <p>
            Only do this if you have confirmed with the administrator of{" "}
            <strong>{confirmForget.changedKey.host}</strong> that its key was replaced.
          </p>
          <dl className="fingerprint">
            <dt>Presented</dt>
            <dd><code>{confirmForget.changedKey.presented}</code></dd>
            {confirmForget.changedKey.known.map((k) => (
              <Fragment key={k}>
                <dt>Recorded</dt>
                <dd><code>{k}</code></dd>
              </Fragment>
            ))}
          </dl>
          <p className="hint">After removing it, reconnect: you will be asked to verify the new key.</p>
        </Dialog>
      )}
      {paletteOpen && (
        <CommandPalette
          onClose={() => setPaletteOpen(false)}
          items={[
            ...tabs.map(
              (t): PaletteItem => ({ id: `tab-${t.key}`, label: t.title, detail: t.kind, group: "Tabs", run: () => setActive(t.key) }),
            ),
            ...sessions.map(
              (s): PaletteItem => ({
                id: `ses-${s.id}`,
                label: s.name,
                detail: `${targetLabel(s)}${s.folder ? ` · ${s.folder}` : ""}${(s.tags ?? []).map((x) => ` #${x}`).join("")}`,
                group: "Sessions",
                run: () => connectSaved(s),
              }),
            ),
            ...(
              [
                ["New local terminal", () => openLocal()],
                ["Quick connect…", () => setSessionDialog({ mode: "quick" })],
                ["New session…", () => setSessionDialog({ mode: "create" })],
                ["Serial console…", () => setSerialOpen(true)],
                ["SSH keys…", () => setKeysOpen(true)],
                ["Settings…", () => setSettingsOpen(true)],
                ["About Termalith", () => setAboutOpen(true)],
                ["Import ~/.ssh/config…", () => setImporting(true)],
                ["Import sessions…", () => setTransferMode("import")],
                ["Export sessions…", () => setTransferMode("export")],
              ] as const
            ).map(([label, run]): PaletteItem => ({ id: `act-${label}`, label, group: "Actions", run })),
          ]}
        />
      )}
      {aboutOpen && <AboutDialog info={info} onClose={() => setAboutOpen(false)} />}
      {crashes.length > 0 && (
        <CrashNotice
          reports={crashes}
          onDismiss={() => {
            void dismissCrashReports(crashes.map((c) => c.id)).catch(() => {});
            setCrashes([]);
          }}
        />
      )}
      {keysOpen && (
        <KeysDialog
          targets={tabs
            .filter((t) => t.kind === "ssh" && t.terminalId !== undefined && !t.ended)
            .map((t) => ({ terminalId: t.terminalId as TerminalId, title: t.title }))}
          onClose={() => setKeysOpen(false)}
        />
      )}
      {confirmDiscard && (
        <Dialog
          title="Discard unsaved changes?"
          tone="warning"
          onCancel={() => setConfirmDiscard(null)}
          actions={
            <>
              <button type="button" onClick={() => setConfirmDiscard(null)}>
                Keep editing
              </button>
              <button
                type="button"
                className="danger"
                onClick={() => {
                  closeTab(confirmDiscard.key);
                  setConfirmDiscard(null);
                }}
              >
                Discard
              </button>
            </>
          }
        >
          <p>
            <strong>{confirmDiscard.title}</strong> has changes that were not saved to the server.
          </p>
        </Dialog>
      )}
      {confirmDelete && (
        <Dialog
          title="Delete session?"
          onCancel={() => setConfirmDelete(null)}
          actions={
            <>
              <button type="button" onClick={() => setConfirmDelete(null)}>
                Cancel
              </button>
              <button
                type="button"
                className="danger"
                onClick={() => {
                  const s = confirmDelete;
                  setConfirmDelete(null);
                  deleteSession(s.id).then(refresh, (e) => setNotice(String(e?.message ?? e)));
                }}
              >
                Delete
              </button>
            </>
          }
        >
          <p>
            Delete <strong>{confirmDelete.name}</strong>? Its saved password, if any, is removed from
            the system keychain. Open terminals stay connected.
          </p>
        </Dialog>
      )}
    </div>
  );
}
