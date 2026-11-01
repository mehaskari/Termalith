import { type FormEvent, useCallback, useEffect, useState } from "react";
import {
  type KeyView,
  type TerminalId,
  generateKey,
  installKey,
  isIpcError,
  listKeys,
  publicKeyText,
} from "../terminal/api";
import { Dialog } from "./Dialog";

interface Props {
  /** Connected SSH tabs a key can be installed on. */
  targets: { terminalId: TerminalId; title: string }[];
  onClose: () => void;
}

const message = (e: unknown) => (isIpcError(e) ? e.message : String(e));

/** Lists, generates and installs SSH keys from ~/.ssh. */
export function KeysDialog({ targets, onClose }: Props) {
  const [keys, setKeys] = useState<KeyView[] | null>(null);
  const [status, setStatus] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [target, setTarget] = useState<TerminalId | "">(targets[0]?.terminalId ?? "");
  const [showGen, setShowGen] = useState(false);

  const refresh = useCallback(() => {
    listKeys().then(setKeys, (e) => setError(message(e)));
  }, []);
  useEffect(refresh, [refresh]);

  async function copy(k: KeyView) {
    setError(null);
    try {
      await navigator.clipboard.writeText(await publicKeyText(k.name));
      setStatus(`Public key ${k.name}.pub copied.`);
    } catch (e) {
      setError(message(e));
    }
  }

  async function install(k: KeyView) {
    if (target === "") return;
    setError(null);
    setStatus(`Installing ${k.name}…`);
    try {
      const r = await installKey(target, k.name);
      setStatus(r.added ? `Added ${k.name} to ${r.path}.` : `${k.name} is already in ${r.path}.`);
    } catch (e) {
      setStatus(null);
      setError(message(e));
    }
  }

  return (
    <Dialog
      title="SSH keys"
      onCancel={onClose}
      actions={
        <button type="button" onClick={onClose}>
          Close
        </button>
      }
    >
      <div className="stack keys">
        {keys === null && !error && <p>Loading…</p>}
        {keys?.length === 0 && <p className="hint">No key pairs in ~/.ssh yet.</p>}
        {keys && keys.length > 0 && (
          <>
            {targets.length > 0 ? (
              <label>
                Install on
                <select value={target} onChange={(e) => setTarget(e.target.value === "" ? "" : Number(e.target.value))}>
                  {targets.map((t) => (
                    <option key={t.terminalId} value={t.terminalId}>
                      {t.title}
                    </option>
                  ))}
                </select>
              </label>
            ) : (
              <p className="hint">Connect an SSH tab to install a key on its server.</p>
            )}
            <ul className="key-list">
              {keys.map((k) => (
                <li key={k.name}>
                  <div className="key-head">
                    <strong>{k.name}</strong>
                    <span className="key-meta">
                      {k.algorithm.replace(/^ssh-/, "")}
                      {k.encrypted === true && " · passphrase"}
                      {!k.hasPrivate && " · public only"}
                    </span>
                  </div>
                  <code className="key-fp">{k.fingerprint}</code>
                  {k.comment && <span className="key-meta">{k.comment}</span>}
                  <div className="key-actions">
                    <button type="button" onClick={() => void copy(k)}>
                      Copy public key
                    </button>
                    <button type="button" disabled={target === ""} onClick={() => void install(k)}>
                      Install on server
                    </button>
                  </div>
                </li>
              ))}
            </ul>
          </>
        )}
        {status && <p role="status">{status}</p>}
        {error && (
          <p className="form-error" role="alert">
            {error}
          </p>
        )}
        {showGen ? (
          <GenerateForm
            taken={keys?.map((k) => k.name) ?? []}
            onDone={(k) => {
              setShowGen(false);
              setStatus(`Created ~/.ssh/${k.name} and ${k.name}.pub.`);
              refresh();
            }}
            onCancel={() => setShowGen(false)}
          />
        ) : (
          <button type="button" className="link" onClick={() => setShowGen(true)}>
            Generate a new key…
          </button>
        )}
      </div>
    </Dialog>
  );
}

function GenerateForm(props: { taken: string[]; onDone: (k: KeyView) => void; onCancel: () => void }) {
  const [name, setName] = useState(() => (props.taken.includes("id_ed25519") ? "termalith_ed25519" : "id_ed25519"));
  const [comment, setComment] = useState("");
  // Held only while the form is open.
  const [pass, setPass] = useState("");
  const [again, setAgain] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const mismatch = pass !== again;

  async function submit(e: FormEvent) {
    e.preventDefault();
    if (mismatch) return;
    setBusy(true);
    setError(null);
    try {
      const k = await generateKey(name.trim(), comment.trim(), pass || null);
      setPass("");
      setAgain("");
      props.onDone(k);
    } catch (err) {
      setError(message(err));
      setBusy(false);
    }
  }

  return (
    <form className="form-grid key-gen" onSubmit={submit}>
      <label className="span-2">
        File name in ~/.ssh
        <input value={name} spellCheck={false} autoCapitalize="off" onChange={(e) => setName(e.target.value)} />
      </label>
      <label className="span-2">
        Comment
        <input value={comment} placeholder="Optional, e.g. me@laptop" onChange={(e) => setComment(e.target.value)} />
      </label>
      <label>
        Passphrase
        <input type="password" autoComplete="new-password" value={pass} onChange={(e) => setPass(e.target.value)} />
      </label>
      <label>
        Repeat
        <input
          type="password"
          autoComplete="new-password"
          value={again}
          aria-invalid={mismatch || undefined}
          onChange={(e) => setAgain(e.target.value)}
        />
      </label>
      <p className="hint span-2">
        Ed25519. Leave the passphrase empty for an unencrypted key (anyone who gets the file can use it).
      </p>
      {error && (
        <p className="form-error span-2" role="alert">
          {error}
        </p>
      )}
      <div className="key-actions span-2">
        <button type="button" onClick={props.onCancel}>
          Cancel
        </button>
        <button type="submit" className="primary" disabled={busy || mismatch || !name.trim()}>
          Generate
        </button>
      </div>
    </form>
  );
}
