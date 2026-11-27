import { type FormEvent, useEffect, useMemo, useState } from "react";
import {
  type DesktopClients,
  type SessionInput,
  type SessionProtocol,
  type SessionRecord,
  desktopClients,
  isIpcError,
} from "../terminal/api";
import { Dialog } from "./Dialog";
import { ProtocolBadge } from "./icons";
import { IconPicker } from "./sessionIcons";
import { TERMINAL_SCHEMES } from "./theme";
import { parseTypedConnection } from "./commandLine";
import { type SessionFormState, allFolderPaths, applyTypedConnection, emptyForm, formFromSession, isFileStore, parseForm, withProtocol } from "./sessionList";

const INSTALL_HINT: Record<"rdp" | "vnc", string> = {
  rdp: "No RDP client is installed, so this session opens inside Termalith (no client needed). To use an external client, install Windows App (macOS) or FreeRDP (xfreerdp, Linux); Windows normally includes Remote Desktop Connection.",
  vnc: "No VNC client is installed, so this session opens inside Termalith (no client needed). To use an external client: Screen Sharing on macOS, TigerVNC or RealVNC Viewer on Windows, TigerVNC (vncviewer) on Linux.",
};

interface Props {
  mode: "create" | "edit" | "quick";
  initial?: SessionInput;
  /** Id of the session being edited (excluded from jump host choices). */
  editingId?: string;
  /** Saved sessions that can serve as a jump host. */
  sessions: SessionRecord[];
  /** Stored folders, offered as suggestions. */
  folders?: string[];
  /** Folder for a new session (created from a folder's menu). */
  defaultFolder?: string;
  /** Persist or connect; reject with an IpcError to show it in the form. */
  onSubmit: (value: SessionInput) => Promise<void>;
  onCancel: () => void;
}

const TITLES = { create: "New session", edit: "Edit session", quick: "Quick connect" };

/** Whether the "Connection options" section holds anything but defaults. */
export function optionsInUse(f: SessionFormState): boolean {
  return (
    !!f.jumpVia || f.filesOnly || f.mosh || !!f.startupCommand.trim() || f.forwardAgent || f.x11Forward || f.legacySsh
  );
}

/** Whether the "Look" section holds anything but defaults. */
export function lookInUse(f: SessionFormState): boolean {
  return !!f.icon || !!f.lookScheme || !!f.lookFontSize.trim() || !!f.lookFont.trim();
}

/** The connection types, with a few words on when to use each. */
export const SESSION_TYPES: Array<{ protocol: SessionProtocol; name: string; note: string }> = [
  { protocol: "ssh", name: "SSH", note: "Terminal and files" },
  { protocol: "rdp", name: "RDP", note: "Windows desktop" },
  { protocol: "vnc", name: "VNC", note: "Remote screen" },
  { protocol: "telnet", name: "Telnet", note: "Old devices, unencrypted" },
  { protocol: "ftps", name: "FTPS", note: "Files, encrypted" },
  { protocol: "ftp", name: "FTP", note: "Files, unencrypted" },
  { protocol: "s3", name: "S3", note: "Object storage" },
];
const SUBMIT = { create: "Save", edit: "Save", quick: "Connect" };

export function SessionDialog({ mode, initial, editingId, sessions, folders = [], defaultFolder, onSubmit, onCancel }: Props) {
  const [form, setForm] = useState<SessionFormState>(
    initial ? formFromSession(initial) : { ...emptyForm(), folder: defaultFolder ?? "" },
  );
  const folderChoices = useMemo(() => allFolderPaths(sessions, folders), [sessions, folders]);
  const [error, setError] = useState<{ field?: string; message: string } | null>(null);
  const [busy, setBusy] = useState(false);
  const set = (patch: Partial<SessionFormState>) => setForm((f) => ({ ...f, ...patch }));
  const invalid = (field: string) => (error?.field === field ? true : undefined);
  const [clients, setClients] = useState<DesktopClients | null>(null);
  const ssh = form.protocol === "ssh";
  const shell = ssh && !form.filesOnly;
  const desktop = form.protocol === "rdp" || form.protocol === "vnc";
  useEffect(() => {
    if (desktop && !clients) desktopClients().then(setClients, () => {});
  }, [desktop, clients]);
  const client = !desktop || !clients ? undefined : clients[form.protocol as "rdp" | "vnc"];
  // Folded sections start open when they hold something; the user opens
  // and closes them, and an error inside opens its section.
  const optionsAvailable = !isFileStore(form) || ssh;
  const [optionsOpen, setOptionsOpen] = useState(() => optionsInUse(form));
  const [lookOpen, setLookOpen] = useState(() => lookInUse(form));

  /** A command typed into Host (`ssh -p 2222 me@host`) fills the form. */
  function readTypedHost(f: SessionFormState): SessionFormState | null {
    const typed = parseTypedConnection(f.host);
    if (!typed) return f;
    if (mode === "quick" && (typed.protocol !== "ssh" || typed.mosh || typed.filesOnly)) {
      setError({ field: "host", message: "Quick connect opens SSH terminals; use New session for other types." });
      return null;
    }
    return applyTypedConnection(f, typed);
  }

  async function submit(e: FormEvent) {
    e.preventDefault();
    const read = readTypedHost(form);
    if (!read) return;
    if (read !== form) setForm(read);
    const parsed = parseForm(read, { requireName: mode !== "quick" });
    if (!parsed.ok) {
      setError({ field: parsed.field, message: parsed.message });
      if (parsed.field === "startupCommand") setOptionsOpen(true);
      if (parsed.field === "lookFontSize") setLookOpen(true);
      return;
    }
    setBusy(true);
    try {
      await onSubmit(parsed.value);
    } catch (err) {
      setError({ message: isIpcError(err) ? err.message : String(err) });
      setBusy(false);
    }
  }

  return (
    <Dialog
      title={TITLES[mode]}
      onCancel={onCancel}
      actions={
        <>
          <button type="button" onClick={onCancel}>
            Cancel
          </button>
          <button type="submit" form="session-form" className="primary" disabled={busy}>
            {SUBMIT[mode]}
          </button>
        </>
      }
    >
      <form id="session-form" className="form-grid" onSubmit={submit} noValidate>
        {mode !== "quick" && (
          <div className="span-2 type-field">
            <span className="field-label" id="session-type-label">
              Type
            </span>
            <div className="type-tiles" role="radiogroup" aria-labelledby="session-type-label">
              {SESSION_TYPES.map((t) => (
                <button
                  key={t.protocol}
                  type="button"
                  role="radio"
                  aria-checked={form.protocol === t.protocol}
                  className={`type-tile ${form.protocol === t.protocol ? "selected" : ""}`}
                  onClick={() => setForm((f) => withProtocol(f, t.protocol))}
                >
                  <ProtocolBadge protocol={t.protocol} size={16} />
                  <span className="type-tile-text">
                    <span className="type-tile-name">{t.name}</span>
                    <span className="type-tile-note">{t.note}</span>
                  </span>
                </button>
              ))}
            </div>
          </div>
        )}
        {desktop && clients && (
          <p className="hint span-2">
            {client
              ? `Opens in ${client.name} (external window).${client.asksPasswordFirst ? " You will be asked for the password when connecting; it is not saved." : " The client asks for credentials itself."}`
              : INSTALL_HINT[form.protocol as "rdp" | "vnc"]}
          </p>
        )}
        {form.protocol === "telnet" && (
          <p className="pane-warning span-2">
            Telnet is not encrypted: everything you type, including passwords, can be read on the network.
            Use it only for devices that offer nothing else. Through a jump host, only the part between the
            jump host and the device is unencrypted.
          </p>
        )}
        {form.protocol === "ftp" && (
          <p className="pane-warning span-2">
            FTP is not encrypted: your password and files can be read on the network. Prefer SFTP where the
            server offers it. Leave the username empty to log in anonymously.
          </p>
        )}
        {form.protocol === "ftps" && (
          <p className="hint span-2">
            Uses explicit TLS on the FTP port. You are shown the server&apos;s certificate the first time;
            the connection fails if the server does not offer TLS. Leave the username empty to log in
            anonymously.
          </p>
        )}
        {form.protocol === "s3" && (
          <p className="hint span-2">
            Host is the endpoint, e.g. <code>s3.eu-central-1.amazonaws.com</code>, <code>&lt;account&gt;.r2.cloudflarestorage.com</code>{" "}
            or your MinIO server. You are asked for the secret access key when connecting; it can be saved in
            the keychain like a password.
          </p>
        )}
        {mode !== "quick" && (
          <label className="span-2">
            Name
            <input
              autoFocus
              value={form.name}
              aria-invalid={invalid("name")}
              onChange={(e) => set({ name: e.target.value })}
            />
          </label>
        )}
        <label className="span-2">
          Host
          <input
            autoFocus={mode === "quick"}
            value={form.host}
            placeholder={mode === "quick" ? "host, user@host:port or ssh -p 2222 user@host" : "server.example.com or 10.0.0.5"}
            onBlur={() => {
              const read = readTypedHost(form);
              if (read && read !== form) setForm(read);
            }}
            spellCheck={false}
            autoCapitalize="off"
            aria-invalid={invalid("host")}
            onChange={(e) => set({ host: e.target.value })}
          />
        </label>
        <label>
          Port
          <input
            value={form.port}
            inputMode="numeric"
            aria-invalid={invalid("port")}
            onChange={(e) => set({ port: e.target.value })}
          />
        </label>
        <label>
          {ssh ? "Username" : form.protocol === "s3" ? "Access key ID" : "Username (optional)"}
          <input
            value={form.username}
            spellCheck={false}
            autoCapitalize="off"
            aria-invalid={invalid("username")}
            onChange={(e) => set({ username: e.target.value })}
          />
        </label>
        {form.protocol === "telnet" && (
          <>
            <label className="checkbox span-2">
              <input
                type="checkbox"
                checked={form.telnetLogin && !!form.username.trim()}
                disabled={!form.username.trim()}
                onChange={(e) => set({ telnetLogin: e.target.checked })}
              />
              Log in automatically (types the user name and password at the device&apos;s prompts)
            </label>
            {form.telnetLogin && form.username.trim() && (
              <p className="hint span-2">
                You are asked for the password when connecting; it can be saved in the keychain, and is saved
                only after the device shows its prompt (# &gt; $). The password still crosses the network
                unencrypted, as with typing it.
              </p>
            )}
          </>
        )}
        {form.protocol === "s3" && (
          <>
            <label>
              Bucket
              <input
                value={form.s3Bucket}
                spellCheck={false}
                autoCapitalize="off"
                aria-invalid={invalid("s3Bucket")}
                onChange={(e) => set({ s3Bucket: e.target.value })}
              />
            </label>
            <label>
              Region (optional)
              <input
                value={form.s3Region}
                placeholder="us-east-1"
                spellCheck={false}
                autoCapitalize="off"
                aria-invalid={invalid("s3Region")}
                onChange={(e) => set({ s3Region: e.target.value })}
              />
            </label>
            <label className="checkbox span-2">
              <input type="checkbox" checked={form.s3PathStyle} onChange={(e) => set({ s3PathStyle: e.target.checked })} />
              Path-style addressing (MinIO, Ceph and most self-hosted servers)
            </label>
            <label className="checkbox span-2">
              <input
                type="checkbox"
                checked={form.s3Https}
                onChange={(e) => set({ s3Https: e.target.checked, port: e.target.checked && form.port === "80" ? "443" : !e.target.checked && form.port === "443" ? "80" : form.port })}
              />
              Use HTTPS (turn off only for a local server without TLS)
            </label>
          </>
        )}
        {form.protocol === "rdp" && (
          <label>
            Domain (optional)
            <input
              value={form.domain}
              spellCheck={false}
              autoCapitalize="off"
              aria-invalid={invalid("domain")}
              onChange={(e) => set({ domain: e.target.value })}
            />
          </label>
        )}
        {ssh && (
          <label>
            Authentication
            <select
              value={form.authKind}
              onChange={(e) => set({ authKind: e.target.value as SessionFormState["authKind"] })}
            >
              <option value="password">Password</option>
              <option value="keyFile">Private key file</option>
              <option value="agent">SSH agent</option>
            </select>
          </label>
        )}
        {ssh && form.authKind === "keyFile" && (
          <label className="span-2">
            Private key file
            <input
              value={form.keyPath}
              placeholder="/Users/me/.ssh/id_ed25519"
              spellCheck={false}
              autoCapitalize="off"
              aria-invalid={invalid("keyPath")}
              onChange={(e) => set({ keyPath: e.target.value })}
            />
          </label>
        )}
        {mode !== "quick" && (
          <label className="span-2">
            Tags
            <input
              value={form.tags}
              placeholder="Optional, comma-separated, e.g. prod, web"
              spellCheck={false}
              autoCapitalize="off"
              aria-invalid={invalid("tags")}
              onChange={(e) => set({ tags: e.target.value })}
            />
          </label>
        )}
        {mode !== "quick" && (
          <label className="span-2">
            Folder
            <input
              value={form.folder}
              list="folder-choices"
              placeholder="None (top level). Subfolders with /, e.g. Production/DB"
              onChange={(e) => set({ folder: e.target.value })}
            />
            <datalist id="folder-choices">
              {folderChoices.map((f) => (
                <option key={f} value={f} />
              ))}
            </datalist>
          </label>
        )}
        {mode !== "quick" && optionsAvailable && (
          <details
            className="span-2 form-section"
            open={optionsOpen}
            onToggle={(e) => setOptionsOpen(e.currentTarget.open)}
          >
            <summary>Connection options</summary>
            <div className="form-grid">
        {!isFileStore(form) && !(form.mosh && shell) && (
          <label className="span-2">
            {desktop ? "Tunnel through (SSH session)" : "Connect via (jump host)"}
            <select value={form.jumpVia} onChange={(e) => set({ jumpVia: e.target.value })}>
              <option value="">Direct connection</option>
              {sessions
                .filter((s) => s.id !== editingId && (s.protocol ?? "ssh") === "ssh")
                .map((s) => (
                  <option key={s.id} value={s.id}>
                    {s.name} ({s.username}@{s.host})
                  </option>
                ))}
            </select>
          </label>
        )}
        {ssh && (
          <label className="checkbox span-2">
            <input type="checkbox" checked={form.filesOnly} onChange={(e) => set({ filesOnly: e.target.checked })} />
            Files only (SFTP, no terminal)
          </label>
        )}
        {shell && (
          <label className="checkbox span-2">
            <input type="checkbox" checked={form.mosh} onChange={(e) => set({ mosh: e.target.checked })} />
            Connect with Mosh (stays connected over changing networks; needs Mosh on this computer and the server)
          </label>
        )}
        {shell && form.mosh && (
          <p className="hint span-2">
            Mosh signs in with your system&apos;s ssh and its known hosts, in the terminal: passwords are typed there and
            not saved. No jump host or forwarding.
          </p>
        )}
        {shell && (
          <label className="span-2">
            Run after login
            <input
              value={form.startupCommand}
              placeholder="Optional, e.g. cd /var/log or tmux attach"
              spellCheck={false}
              autoCapitalize="off"
              aria-invalid={invalid("startupCommand")}
              onChange={(e) => set({ startupCommand: e.target.value })}
            />
          </label>
        )}
        {shell && (
          <label className="checkbox span-2">
            <input
              type="checkbox"
              checked={form.forwardAgent}
              onChange={(e) => set({ forwardAgent: e.target.checked })}
            />
            Forward my SSH agent to this host
          </label>
        )}
        {shell && (
          <label className="checkbox span-2">
            <input type="checkbox" checked={form.x11Forward} onChange={(e) => set({ x11Forward: e.target.checked })} />
            Forward X11 (show the server&apos;s graphical programs here)
          </label>
        )}
        {shell && form.x11Forward && (
          <p className="hint span-2">
            Needs a local X server: XQuartz on macOS, VcXsrv or Xming on Windows. Programs on the server can
            then open windows on your screen.
          </p>
        )}
        {shell && form.forwardAgent && (
          <p className="hint span-2">
            Only for hosts you trust: while connected, anyone with root access on the server can use your
            agent&apos;s keys (they cannot copy them). Not applied to the jump host.
          </p>
        )}
        {form.protocol === "ssh" && !form.mosh && (
          <label className="checkbox span-2">
            <input type="checkbox" checked={form.legacySsh} onChange={(e) => set({ legacySsh: e.target.checked })} />
            Allow old SSH algorithms (for old switches and routers; weaker security)
          </label>
        )}
            </div>
          </details>
        )}
        {(
          <details className="span-2 form-section" open={lookOpen} onToggle={(e) => setLookOpen(e.currentTarget.open)}>
            <summary>Look: icon, colors and font</summary>
            <div className="form-grid">
        {(shell || form.protocol === "telnet") && (
          <fieldset className="span-2 session-look">
            <legend>Appearance (optional; empty follows Settings)</legend>
            <div className="form-grid">
              <label>
                Colors
                <select value={form.lookScheme} onChange={(e) => set({ lookScheme: e.target.value })}>
                  <option value="">As in Settings</option>
                  {TERMINAL_SCHEMES.map((sc) => (
                    <option key={sc.id} value={sc.id}>
                      {sc.name}
                    </option>
                  ))}
                </select>
              </label>
              <label>
                Font size
                <input
                  value={form.lookFontSize}
                  inputMode="numeric"
                  placeholder="As in Settings"
                  aria-invalid={invalid("lookFontSize")}
                  onChange={(e) => set({ lookFontSize: e.target.value })}
                />
              </label>
              <label className="span-2">
                Font
                <input
                  value={form.lookFont}
                  placeholder="As in Settings"
                  spellCheck={false}
                  onChange={(e) => set({ lookFont: e.target.value })}
                />
              </label>
            </div>
          </fieldset>
        )}
        {(
          <fieldset className="span-2 session-icon-field">
            <legend>Icon</legend>
            <IconPicker value={form.icon} onChange={(icon) => set({ icon })} automaticLabel="By type" />
          </fieldset>
        )}
            </div>
          </details>
        )}
        {error && (
          <p className="form-error span-2" role="alert">
            {error.message}
          </p>
        )}
      </form>
    </Dialog>
  );
}
