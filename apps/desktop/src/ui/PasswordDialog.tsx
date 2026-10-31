import { type FormEvent, useState } from "react";
import { Dialog } from "./Dialog";

export interface PasswordAnswer {
  password: string;
  save: boolean;
}

interface Props {
  target: string;
  kind: "password" | "passphrase";
  /** Offer "save" only for saved sessions when the OS store works. */
  canSave: boolean;
  /** Why we are asking again, if this is not the first prompt. */
  retry: "wrong" | "savedRejected" | null;
  onSubmit: (answer: PasswordAnswer) => void;
  onCancel: () => void;
}

export function PasswordDialog({ target, kind, canSave, retry, onSubmit, onCancel }: Props) {
  // Held only while the dialog is open; handed to the backend once.
  const [password, setPassword] = useState("");
  const [save, setSave] = useState(false);

  function submit(e: FormEvent) {
    e.preventDefault();
    onSubmit({ password, save: canSave && save });
    setPassword("");
  }

  return (
    <Dialog
      title={kind === "password" ? "Password required" : "Key passphrase required"}
      onCancel={onCancel}
      actions={
        <>
          <button type="button" onClick={onCancel}>
            Cancel
          </button>
          <button type="submit" form="password-form" className="primary" disabled={!password}>
            Connect
          </button>
        </>
      }
    >
      <form id="password-form" onSubmit={submit} className="stack">
        <p>
          {retry === "wrong" && `That ${kind} was rejected. `}
          {retry === "savedRejected" && `The saved ${kind} was rejected. `}
          Enter the {kind} for <strong>{target}</strong>.
        </p>
        <input
          type="password"
          autoFocus
          autoComplete="off"
          value={password}
          onChange={(e) => setPassword(e.target.value)}
          aria-label={kind}
        />
        {canSave && (
          <label className="checkbox">
            <input type="checkbox" checked={save} onChange={(e) => setSave(e.target.checked)} />
            Save in the system keychain
          </label>
        )}
      </form>
    </Dialog>
  );
}
