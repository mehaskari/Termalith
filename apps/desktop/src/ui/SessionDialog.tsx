import { type FormEvent, useState } from "react";
import { type SessionInput, isIpcError } from "../terminal/api";
import { Dialog } from "./Dialog";
import { type SessionFormState, emptyForm, formFromSession, parseForm } from "./sessionList";

interface Props {
  mode: "create" | "edit" | "quick";
  initial?: SessionInput;
  /** Persist or connect; reject with an IpcError to show it in the form. */
  onSubmit: (value: SessionInput) => Promise<void>;
  onCancel: () => void;
}

const TITLES = { create: "New session", edit: "Edit session", quick: "Quick connect" };
const SUBMIT = { create: "Save", edit: "Save", quick: "Connect" };

export function SessionDialog({ mode, initial, onSubmit, onCancel }: Props) {
  const [form, setForm] = useState<SessionFormState>(initial ? formFromSession(initial) : emptyForm());
  const [error, setError] = useState<{ field?: string; message: string } | null>(null);
  const [busy, setBusy] = useState(false);
  const set = (patch: Partial<SessionFormState>) => setForm((f) => ({ ...f, ...patch }));
  const invalid = (field: string) => (error?.field === field ? true : undefined);

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
          Username
          <input
            value={form.username}
            spellCheck={false}
            autoCapitalize="off"
            aria-invalid={invalid("username")}
            onChange={(e) => set({ username: e.target.value })}
          />
        </label>
        <label>
          Authentication
          <select
            value={form.authKind}
            onChange={(e) => set({ authKind: e.target.value as SessionFormState["authKind"] })}
          >
            <option value="password">Password</option>
            <option value="keyFile">Private key file</option>
          </select>
        </label>
        {form.authKind === "keyFile" && (
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
