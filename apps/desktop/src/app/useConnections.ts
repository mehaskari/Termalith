import { useCallback } from "react";
import {
  type IpcError,
  type Opener,
  type TerminalId,
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
import type { LegacyAnswer, LegacyQuestion } from "../ui/LegacyAlgorithmsDialog";
import type { PasswordAnswer } from "../ui/PasswordDialog";
import { isFileStore, isFtp } from "../ui/sessionList";
import type { Tab } from "./tabs";
import type { PasswordQuestion } from "./usePasswordPrompts";

export interface LoginOptions {
  canSave: boolean;
  keyAuth: boolean;
  savedSecret: boolean;
  /** A saved session: "always allow old algorithms" can be kept on it. */
  saved?: boolean;
}

/** The question to ask when a server knows only old SSH algorithms (ADR-043), else null. */
export function legacyQuestion(e: IpcError, target: string, saved: boolean): LegacyQuestion | null {
  if (e.code !== "legacy_algorithms") return null;
  const d = (e.detail ?? {}) as { kind?: unknown; offered?: unknown };
  const offered = Array.isArray(d.offered) ? d.offered.filter((x): x is string => typeof x === "string") : [];
  return { target, kind: typeof d.kind === "string" ? d.kind : "", offered, canRemember: saved };
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
  askLegacy: (q: LegacyQuestion) => Promise<LegacyAnswer | null>;
  /** Reloads the session list (a password was saved). */
  refresh: () => void;
  notify: (message: string) => void;
  canSave: boolean;
  embeddedDesktops: boolean;
}) {
  const { addTab, patchTab, renameTab, askPassword, askLegacy, refresh, notify, canSave, embeddedDesktops } = deps;

  /**
   * Runs a login (`attempt`), asking for a password or key passphrase when
   * the backend needs one, or whether to allow old algorithms for an old
   * device, and retrying; saving happens only after success.
   */
  const withLogin = useCallback(
    async <T extends { saveError?: string | null },>(
      label: string,
      opts: LoginOptions,
      attempt: (password: string | undefined, save: boolean, legacy: LegacyAnswer | undefined) => Promise<T>,
    ): Promise<T> => {
      let secret: string | undefined;
      let save = false;
      let asked = false;
      let legacy: LegacyAnswer | undefined;
      for (;;) {
        try {
          const r = await attempt(secret, save, legacy);
          if (save || legacy === "always") refresh();
          if (r.saveError) notify(`Connected, but the password was not saved: ${r.saveError}`);
          return r;
        } catch (e) {
          if (!isIpcError(e)) throw e;
          const old = legacy ? null : legacyQuestion(e, label, !!opts.saved);
          if (old) {
            const answer = await askLegacy(old);
            if (!answer) throw cancelled();
            legacy = answer;
            continue;
          }
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
    [askPassword, askLegacy, refresh, notify],
  );

  const connect = useCallback(
    (target: SshTarget, label: string, opts: LoginOptions & { startup?: string | null }) => {
      let tabKey = 0;
      const open: Opener = async (cols, rows, onOutput, onExit) => {
        const r = await withLogin(label, { ...opts, saved: "sessionId" in target }, (password, savePassword, legacy) =>
          openSsh({ ...target, password, savePassword, legacy }, cols, rows, onOutput, onExit),
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
        { canSave, keyAuth: s.auth.kind === "keyFile", savedSecret: s.hasSavedSecret, saved: true },
        (password, savePassword, legacy) =>
          (s3 ? openS3Files : ftp ? openFtpFiles : openSshFiles)({ sessionId: s.id, password, savePassword, legacy }),
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
        const open: Opener = s.telnetLogin
          ? (cols, rows, onOutput, onExit) =>
              // The password is saved later, once the device shows its prompt.
              withLogin<{ id: TerminalId; saveError?: null }>(s.name, { canSave, keyAuth: false, savedSecret: s.hasSavedSecret, saved: true }, async (password, save) => ({
                id: await telnetTerminal(s.id, password, save)(cols, rows, onOutput, onExit),
              })).then((r) => r.id)
          : telnetTerminal(s.id);
        const key = addTab(s.name, "telnet", open);
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
    [addTab, canSave, connect, openFilesTab, patchTab, runDesktop, embeddedDesktops, notify, withLogin],
  );

  return { withLogin, connect, openFilesTab, runDesktop, connectSaved };
}
