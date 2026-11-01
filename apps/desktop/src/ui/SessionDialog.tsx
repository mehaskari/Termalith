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
  const desktop = form.protocol !== "ssh";
  useEffect(() => {
    if (desktop && !clients) desktopClients().then(setClients, () => {});
  }, [desktop, clients]);
  const client = form.protocol === "ssh" || !clients ? undefined : clients[form.protocol];

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
            </select>
          </label>
        )}
        {desktop && clients && form.protocol !== "ssh" && (
          <p className="hint span-2">
            {client
              ? `Opens in ${client.name} (external window).${client.asksPasswordFirst ? " You will be asked for the password when connecting; it is not saved." : " The client asks for credentials itself."}`
              : INSTALL_HINT[form.protocol]}
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
          {desktop ? "Username (optional)" : "Username"}
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
        {!desktop && (
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
        {!desktop && form.authKind === "keyFile" && (
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
