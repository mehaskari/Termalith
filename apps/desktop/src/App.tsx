import { Fragment, useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { TerminalView } from "./terminal/TerminalView";
import {
  type AppInfo,
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
  createSession,
  deleteSession,
  forgetHostKey,
  forgetSessionSecret,
  isIpcError,
  listSessions,
  openLocalTerminal,
  openSsh,
  respondHostKey,
  splitTerminal,
  updateSession,
} from "./terminal/api";
import { Dialog } from "./ui/Dialog";
import { FilesPanel } from "./ui/FilesPanel";
import { HostKeyDialog } from "./ui/HostKeyDialog";
import { ImportDialog } from "./ui/ImportDialog";
import { type PasswordAnswer, PasswordDialog } from "./ui/PasswordDialog";
import { SessionDialog } from "./ui/SessionDialog";
import { Sidebar } from "./ui/Sidebar";
import { targetLabel } from "./ui/sessionList";

interface Tab {
  key: number;
  title: string;
  kind: "local" | "ssh";
  open: Opener;
  terminalId?: TerminalId;
  /** Connection over (failed, or the remote shell exited). */
  ended?: boolean;
  filesOpen?: boolean;
  /** Set when the connection was refused because the host key changed. */
  changedKey?: { host: string; port: number; presented: string; known: string[] };
  reconnect?: () => void;
  /** Extra panes beside the main terminal. */
  splits: SplitPane[];
  direction: "row" | "column";
}

interface SplitPane {
  key: number;
  open: Opener;
}

type SessionDialogState =
  | { mode: "create" }
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
  const [sessions, setSessions] = useState<SessionRecord[]>([]);
  const [tabs, setTabs] = useState<Tab[]>([]);
  const [active, setActive] = useState<number | null>(null);
  const [sessionDialog, setSessionDialog] = useState<SessionDialogState | null>(null);
  const [passwordRequest, setPasswordRequest] = useState<PasswordRequest | null>(null);
  const [hostKeyPrompts, setHostKeyPrompts] = useState<HostKeyPrompt[]>([]);
  const [confirmDelete, setConfirmDelete] = useState<SessionRecord | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [dropped, setDropped] = useState<DroppedFile[] | null>(null);
  const [confirmForget, setConfirmForget] = useState<Tab | null>(null);
  const [importing, setImporting] = useState(false);
  const activeRef = useRef<Tab | undefined>(undefined);
  const nextKey = useRef(1);

  const refresh = useCallback(() => {
    listSessions().then(setSessions, (e) => setNotice(`Could not load sessions: ${String(e?.message ?? e)}`));
  }, []);

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
    const pane = { key: nextKey.current++, open: splitTerminal(tab.terminalId) };
    setTabs((t) =>
      t.map((x) => (x.key === tab.key ? { ...x, direction, splits: [...x.splits, pane] } : x)),
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

  const openLocal = useCallback(() => addTab("Local", "local", openLocalTerminal), [addTab]);

  const askPassword = useCallback(
    (req: Omit<PasswordRequest, "resolve">) =>
      new Promise<PasswordAnswer | null>((resolve) => setPasswordRequest({ ...req, resolve })),
    [],
  );

  const connect = useCallback(
    (target: SshTarget, label: string, opts: { canSave: boolean; keyAuth: boolean; savedSecret: boolean }) => {
      let tabKey = 0;
      const open: Opener = async (cols, rows, onOutput, onExit) => {
        let secret: string | undefined;
        let save = false;
        let retry = false;
        for (;;) {
          try {
            const r = await openSsh(
              { ...target, password: secret, savePassword: save },
              cols,
              rows,
              onOutput,
              onExit,
            );
            secret = undefined;
            renameTab(tabKey, r.label);
            if (save) refresh();
            return r.id;
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
              retry: retry
                ? "wrong"
                : e.code === "password_required" && opts.savedSecret
                  ? "savedRejected"
                  : null,
            });
            if (!answer) throw cancelled();
            secret = answer.password;
            save = answer.save;
            retry = true;
          }
        }
      };
      tabKey = addTab(label, "ssh", open);
      patchTab(tabKey, { reconnect: () => connect(target, label, opts) });
    },
    [addTab, askPassword, refresh, renameTab, patchTab],
  );

  const connectSaved = useCallback(
    (s: SessionRecord) =>
      connect({ sessionId: s.id }, s.name, {
        canSave: info?.secretsAvailable ?? false,
        keyAuth: s.auth.kind === "keyFile",
        savedSecret: s.hasSavedSecret,
      }),
    [connect, info],
  );

  const didOpenInitialTab = useRef(false);
  useEffect(() => {
    appInfo().then(setInfo, () => setInfo({ version: "?", secretsAvailable: false, agentAvailable: false }));
    refresh();
    // StrictMode runs effects twice in development; open one tab only.
    if (!didOpenInitialTab.current) {
      didOpenInitialTab.current = true;
      openLocal();
    }
    const unlisten = listen<HostKeyPrompt>("host-key-prompt", (e) =>
      setHostKeyPrompts((q) => [...q, e.payload]),
    );
    const unlistenDrop = listen<DroppedFile[]>("files-dropped", (e) => {
      const tab = activeRef.current;
      if (tab?.filesOpen && tab.terminalId !== undefined) setDropped(e.payload);
      else setNotice("To upload, open Files on a connected SSH tab and drop the files again.");
    });
    return () => {
      void unlisten.then((f) => f());
      void unlistenDrop.then((f) => f());
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
        onConnect={connectSaved}
        onEdit={(s) => setSessionDialog({ mode: "edit", session: s })}
        onDelete={setConfirmDelete}
        onForgetSecret={(s) =>
          forgetSessionSecret(s.id).then(refresh, (e) => setNotice(String(e?.message ?? e)))
        }
        onNew={() => setSessionDialog({ mode: "create" })}
        onQuickConnect={() => setSessionDialog({ mode: "quick" })}
        onLocalTerminal={openLocal}
        onImport={() => setImporting(true)}
      />
      <main className="workspace">
        <div className="tabbar" role="tablist">
          {tabs.map((t) => (
            <div key={t.key} className={`tab ${t.key === active ? "active" : ""}`} role="presentation">
              <button
                type="button"
                role="tab"
                aria-selected={t.key === active}
                className="tab-title"
                onClick={() => setActive(t.key)}
              >
                <span className={`tab-kind ${t.kind}`} aria-hidden="true" />
                {t.title}
              </button>
              <button type="button" className="tab-close" aria-label={`Close ${t.title}`} onClick={() => closeTab(t.key)}>
                ×
              </button>
            </div>
          ))}
          <button type="button" className="tab-new" aria-label="New local terminal" title="New local terminal" onClick={openLocal}>
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
            <div key={t.key} className="pane" hidden={t.key !== active} role="tabpanel">
              <div className="pane-toolbar">
                  {t.changedKey ? (
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
                      disabled={t.terminalId === undefined || t.ended}
                      onClick={() => splitTab(t, "row")}
                    >
                      ◫
                    </button>
                    <button
                      type="button"
                      title="Split top and bottom"
                      aria-label="Split top and bottom"
                      disabled={t.terminalId === undefined || t.ended}
                      onClick={() => splitTab(t, "column")}
                    >
                      ⊟
                    </button>
                    {t.reconnect && t.ended && (
                      <button type="button" onClick={() => { closeTab(t.key); t.reconnect?.(); }}>
                        Reconnect
                      </button>
                    )}
                    {t.kind === "ssh" && (
                      <button
                        type="button"
                        aria-pressed={!!t.filesOpen}
                        disabled={t.terminalId === undefined || t.ended}
                        onClick={() => patchTab(t.key, { filesOpen: !t.filesOpen })}
                      >
                        Files
                      </button>
                    )}
                  </span>
              </div>
              <div className="pane-body">
                <div className={`pane-terminals ${t.direction}`}>
                  <div className="pane-terminal">
                    <TerminalView
                      open={t.open}
                      active={t.key === active}
                      onReady={(id) => patchTab(t.key, { terminalId: id })}
                      onFailed={(e) => {
                        patchTab(t.key, { ended: true });
                        onTabFailed(t, e);
                      }}
                      onExited={() => patchTab(t.key, { ended: true, filesOpen: false })}
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
                      <TerminalView open={s.open} active={t.key === active} />
                    </div>
                  ))}
                </div>
                {t.filesOpen && t.terminalId !== undefined && (
                  <FilesPanel
                    terminalId={t.terminalId}
                    dropped={t.key === active ? dropped : null}
                    onDroppedHandled={() => setDropped(null)}
                    onClose={() => patchTab(t.key, { filesOpen: false })}
                  />
                )}
              </div>
            </div>
          ))}
        </div>
      </main>

      {sessionDialog && (
        <SessionDialog
          mode={sessionDialog.mode}
          initial={sessionDialog.mode === "edit" ? sessionDialog.session : undefined}
          editingId={sessionDialog.mode === "edit" ? sessionDialog.session.id : undefined}
          sessions={sessions}
          onSubmit={submitSession}
          onCancel={() => setSessionDialog(null)}
        />
      )}
      {passwordRequest && (
        <PasswordDialog
          target={passwordRequest.target}
          kind={passwordRequest.kind}
          canSave={passwordRequest.canSave}
          retry={passwordRequest.retry}
          onSubmit={(a) => {
            setPasswordRequest(null);
            passwordRequest.resolve(a);
          }}
          onCancel={() => {
            setPasswordRequest(null);
            passwordRequest.resolve(null);
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
