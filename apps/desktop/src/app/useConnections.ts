import { useCallback } from "react";
import {
  type IpcError,
  type Opener,
  type SessionRecord,
  type SshTarget,
  desktopClients,
  isIpcError,
  moshTerminal,
  openDesktop,
  openFtpFiles,
  openLocalTerminal,
  openS3Files,
  openSsh,
  openSshFiles,
  telnetTerminal,
} from "../terminal/api";
import type { PasswordAnswer } from "../ui/PasswordDialog";
import { isFileStore, isFtp } from "../ui/sessionList";
import type { Tab } from "./tabs";
import type { PasswordQuestion } from "./usePasswordPrompts";

export interface LoginOptions {
  canSave: boolean;
  keyAuth: boolean;
  savedSecret: boolean;
}

/**
 * What a failed login attempt should ask for: a password, a key
 * passphrase, or nothing (the error is final). `triedSecret`: the attempt
 * already used a password the user typed.
 */
export function loginQuestion(
  e: IpcError,
  triedSecret: boolean,
  opts: Pick<LoginOptions, "keyAuth">,
): "password" | "passphrase" | null {
  if (opts.keyAuth && e.code === "key_error") return "passphrase";
  if (e.code === "password_required") return "password";
  if (e.code === "auth_failed" && triedSecret && !opts.keyAuth) return "password";
  return null;
}

/** Why the dialog asks again: a wrong answer, or a saved password the server refused. */
export function retryReason(e: IpcError, askedBefore: boolean, savedSecret: boolean): PasswordQuestion["retry"] {
  if (askedBefore) return "wrong";
  return e.code === "password_required" && savedSecret ? "savedRejected" : null;
}

const cancelled = () => ({ code: "cancelled", message: "Connection cancelled" });

/**
 * Opening connections in tabs: SSH terminals, file browsers (SFTP, FTP, S3),
 * Telnet, Mosh and remote desktops, with password questions and retries.
 * Passwords are saved only after a successful login.
 */
export function useConnections(deps: {
  addTab: (title: string, kind: Tab["kind"], open: Opener) => number;
  patchTab: (key: number, patch: Partial<Tab>) => void;
  renameTab: (key: number, title: string) => void;
  askPassword: (q: PasswordQuestion) => Promise<PasswordAnswer | null>;
  /** Reloads the session list (a password was saved). */
  refresh: () => void;
  notify: (message: string) => void;
  canSave: boolean;
  embeddedDesktops: boolean;
}) {
  const { addTab, patchTab, renameTab, askPassword, refresh, notify, canSave, embeddedDesktops } = deps;

  /**
   * Runs a login (`attempt`), asking for a password or key passphrase when
   * the backend needs one and retrying; saving happens only after success.
   */
  const withLogin = useCallback(
    async <T extends { saveError?: string | null },>(
      label: string,
      opts: LoginOptions,
      attempt: (password: string | undefined, save: boolean) => Promise<T>,
    ): Promise<T> => {
      let secret: string | undefined;
      let save = false;
      let asked = false;
      for (;;) {
        try {
          const r = await attempt(secret, save);
          if (save) refresh();
          if (r.saveError) notify(`Connected, but the password was not saved: ${r.saveError}`);
          return r;
        } catch (e) {
          if (!isIpcError(e)) throw e;
          const kind = loginQuestion(e, secret !== undefined, opts);
          if (!kind) throw e;
          const answer = await askPassword({
            target: label,
            kind,
            canSave: opts.canSave,
            retry: retryReason(e, asked, opts.savedSecret),
          });
          if (!answer) throw cancelled();
          secret = answer.password;
          save = answer.save;
          asked = true;
        }
      }
    },
    [askPassword, refresh, notify],
  );

  const connect = useCallback(
    (target: SshTarget, label: string, opts: LoginOptions & { startup?: string | null }) => {
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
      return tabKey;
    },
    [addTab, withLogin, renameTab, patchTab],
  );

  /** A file browser tab on a saved session (SFTP, FTP or S3; no shell). */
  const openFilesTab = useCallback(
    (s: SessionRecord) => {
      const ftp = isFtp(s);
      const s3 = s.protocol === "s3";
      const key = addTab(`${s.name} (files)`, "files", openLocalTerminal);
      patchTab(key, { sessionId: s.id, noShell: ftp || s3 });
      withLogin(
        s.name,
        { canSave, keyAuth: s.auth.kind === "keyFile", savedSecret: s.hasSavedSecret },
        (password, savePassword) =>
          (s3 ? openS3Files : ftp ? openFtpFiles : openSshFiles)({ sessionId: s.id, password, savePassword }),
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

  /** Opens a saved session; returns the tab key when it opened a terminal tab. */
  const connectSaved = useCallback(
    (s: SessionRecord): number | undefined => {
      if (s.protocol === "telnet") {
        const key = addTab(s.name, "telnet", telnetTerminal(s.id));
        patchTab(key, { sessionId: s.id, reconnect: () => connectSaved(s) });
        return key;
      }
      if (s.protocol === "rdp" || s.protocol === "vnc") {
        const protocol = s.protocol;
        const embedded = () => {
          const key = addTab(s.name, "rdp", openLocalTerminal);
          patchTab(key, { sessionId: s.id });
        };
        if (embeddedDesktops) {
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
              notify(
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
      if (s.filesOnly || isFileStore(s)) {
        openFilesTab(s);
        return;
      }
      if (s.mosh) {
        const key = addTab(s.name, "local", moshTerminal(s.id));
        patchTab(key, { sessionId: s.id, startup: s.startupCommand ?? undefined });
        return key;
      }
      return connect({ sessionId: s.id }, s.name, {
        canSave,
        keyAuth: s.auth.kind === "keyFile",
        savedSecret: s.hasSavedSecret,
        startup: s.startupCommand,
      });
    },
    [addTab, canSave, connect, openFilesTab, patchTab, runDesktop, embeddedDesktops, notify],
  );

  return { withLogin, connect, openFilesTab, runDesktop, connectSaved };
}
