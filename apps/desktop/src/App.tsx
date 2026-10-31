import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { TerminalView } from "./terminal/TerminalView";
import {
  type AppInfo,
  type HostKeyAnswer,
  type HostKeyPrompt,
  type Opener,
  type SessionInput,
  type SessionRecord,
  type SshTarget,
  appInfo,
  createSession,
  deleteSession,
  forgetSessionSecret,
  isIpcError,
  listSessions,
  openLocalTerminal,
  openSsh,
  respondHostKey,
  updateSession,
} from "./terminal/api";
import { Dialog } from "./ui/Dialog";
import { HostKeyDialog } from "./ui/HostKeyDialog";
import { type PasswordAnswer, PasswordDialog } from "./ui/PasswordDialog";
import { SessionDialog } from "./ui/SessionDialog";
import { Sidebar } from "./ui/Sidebar";
import { targetLabel } from "./ui/sessionList";

interface Tab {
  key: number;
  title: string;
  kind: "local" | "ssh";
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
  const nextKey = useRef(1);

  const refresh = useCallback(() => {
    listSessions().then(setSessions, (e) => setNotice(`Could not load sessions: ${String(e?.message ?? e)}`));
  }, []);

  const addTab = useCallback((title: string, kind: Tab["kind"], open: Opener) => {
    const key = nextKey.current++;
    setTabs((t) => [...t, { key, title, kind, open }]);
    setActive(key);
    return key;
  }, []);

  const renameTab = useCallback((key: number, title: string) => {
    setTabs((t) => t.map((x) => (x.key === key ? { ...x, title } : x)));
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
    },
    [addTab, askPassword, refresh, renameTab],
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
    appInfo().then(setInfo, () => setInfo({ version: "?", secretsAvailable: false }));
    refresh();
    // StrictMode runs effects twice in development; open one tab only.
    if (!didOpenInitialTab.current) {
      didOpenInitialTab.current = true;
      openLocal();
    }
    const unlisten = listen<HostKeyPrompt>("host-key-prompt", (e) =>
      setHostKeyPrompts((q) => [...q, e.payload]),
    );
    return () => {
      void unlisten.then((f) => f());
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
              <TerminalView open={t.open} active={t.key === active} />
            </div>
          ))}
        </div>
      </main>

      {sessionDialog && (
        <SessionDialog
          mode={sessionDialog.mode}
          initial={sessionDialog.mode === "edit" ? sessionDialog.session : undefined}
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
