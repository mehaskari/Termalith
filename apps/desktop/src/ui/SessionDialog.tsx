import { type FormEvent, useEffect, useState } from "react";
import {
  type DesktopClients,
  type SessionInput,
  type SessionProtocol,
  type SessionRecord,
  desktopClients,
  isIpcError,
} from "../terminal/api";
import { Dialog } from "./Dialog";
import { type SessionFormState, emptyForm, formFromSession, parseForm, withProtocol } from "./sessionList";

const INSTALL_HINT: Record<"rdp" | "vnc", string> = {
  rdp: "No RDP client found. Install Windows App (macOS), or FreeRDP (xfreerdp) on Linux; Windows has Remote Desktop Connection built in.",
  vnc: "No VNC client found. macOS includes Screen Sharing; on Linux or Windows install TigerVNC (vncviewer).",
};

interface Props {
  mode: "create" | "edit" | "quick";
  initial?: SessionInput;
  /** Id of the session being edited (excluded from jump host choices). */
  editingId?: string;
  /** Saved sessions that can serve as a jump host. */
  sessions: SessionRecord[];
  /** Persist or connect; reject with an IpcError to show it in the form. */
  onSubmit: (value: SessionInput) => Promise<void>;
  onCancel: () => void;
}

const TITLES = { create: "New session", edit: "Edit session", quick: "Quick connect" };
const SUBMIT = { create: "Save", edit: "Save", quick: "Connect" };

export function SessionDialog({ mode, initial, editingId, sessions, onSubmit, onCancel }: Props) {
  const [form, setForm] = useState<SessionFormState>(initial ? formFromSession(initial) : emptyForm());
  const [error, setError] = useState<{ field?: string; message: string } | null>(null);
  const [busy, setBusy] = useState(false);
  const set = (patch: Partial<SessionFormState>) => setForm((f) => ({ ...f, ...patch }));
  const invalid = (field: string) => (error?.field === field ? true : undefined);
  const [clients, setClients] = useState<DesktopClients | null>(null);
  const ssh = form.protocol === "ssh";
  const desktop = form.protocol === "rdp" || form.protocol === "vnc";
  useEffect(() => {
    if (desktop && !clients) desktopClients().then(setClients, () => {});
  }, [desktop, clients]);
  const client = !desktop || !clients ? undefined : clients[form.protocol as "rdp" | "vnc"];

  async function submit(e: FormEvent) {
    e.preventDefault();
    const parsed = parseForm(form, { requireName: mode !== "quick" });
    if (!parsed.ok) {
      setError({ field: parsed.field, message: parsed.message });
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
          <label className="span-2">
            Type
            <select
              value={form.protocol}
              onChange={(e) => setForm((f) => withProtocol(f, e.target.value as SessionProtocol))}
            >
              <option value="ssh">SSH terminal</option>
              <option value="rdp">RDP remote desktop</option>
              <option value="vnc">VNC remote desktop</option>
              <option value="telnet">Telnet (unencrypted)</option>
            </select>
          </label>
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
            placeholder="server.example.com or 10.0.0.5"
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
          {ssh ? "Username" : "Username (optional)"}
          <input
            value={form.username}
            spellCheck={false}
            autoCapitalize="off"
            aria-invalid={invalid("username")}
            onChange={(e) => set({ username: e.target.value })}
          />
        </label>
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
        {mode !== "quick" && ssh && (
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
        {mode !== "quick" && ssh && (
          <label className="checkbox span-2">
            <input
              type="checkbox"
              checked={form.forwardAgent}
              onChange={(e) => set({ forwardAgent: e.target.checked })}
            />
            Forward my SSH agent to this host
          </label>
        )}
        {mode !== "quick" && ssh && (
          <label className="checkbox span-2">
            <input type="checkbox" checked={form.x11Forward} onChange={(e) => set({ x11Forward: e.target.checked })} />
            Forward X11 (show the server&apos;s graphical programs here)
          </label>
        )}
        {mode !== "quick" && ssh && form.x11Forward && (
          <p className="hint span-2">
            Needs a local X server: XQuartz on macOS, VcXsrv or Xming on Windows. Programs on the server can
            then open windows on your screen.
          </p>
        )}
        {mode !== "quick" && ssh && form.forwardAgent && (
          <p className="hint span-2">
            Only for hosts you trust: while connected, anyone with root access on the server can use your
            agent&apos;s keys (they cannot copy them). Not applied to the jump host.
          </p>
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
              placeholder="Optional, e.g. Production"
              onChange={(e) => set({ folder: e.target.value })}
            />
          </label>
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
