import { useEffect, useState } from "react";
import { type ImportCandidate, isIpcError, sshConfigImport, sshConfigPreview } from "../terminal/api";
import { Dialog } from "./Dialog";
import { targetLabel } from "./sessionList";

interface Props {
  onDone: (summary: string) => void;
  onCancel: () => void;
}

export function ImportDialog({ onDone, onCancel }: Props) {
  const [candidates, setCandidates] = useState<ImportCandidate[] | null>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    sshConfigPreview().then(
      (c) => {
        setCandidates(c);
        setSelected(new Set(c.map((x) => x.alias)));
      },
      (e) => setError(isIpcError(e) ? e.message : String(e)),
    );
  }, []);

  function toggle(alias: string) {
    setSelected((s) => {
      const n = new Set(s);
      if (n.has(alias)) n.delete(alias);
      else n.add(alias);
      return n;
    });
  }

  async function run() {
    setBusy(true);
    try {
      const r = await sshConfigImport([...selected]);
      const parts = [`Imported ${r.created.length} session(s)`];
      if (r.skipped.length) parts.push(`skipped ${r.skipped.length} that already exist (${r.skipped.join(", ")})`);
      if (r.unresolvedJumps.length) parts.push(`jump host not found for ${r.unresolvedJumps.join(", ")}`);
      onDone(parts.join("; ") + ".");
    } catch (e) {
      setError(isIpcError(e) ? e.message : String(e));
      setBusy(false);
    }
  }

  return (
    <Dialog
      title="Import from ~/.ssh/config"
      onCancel={onCancel}
      actions={
        <>
          <button type="button" onClick={onCancel}>
            Cancel
          </button>
          <button type="button" className="primary" disabled={busy || selected.size === 0} onClick={() => void run()}>
            Import {selected.size > 0 ? selected.size : ""}
          </button>
        </>
      }
    >
      {error && <p className="form-error" role="alert">{error}</p>}
      {candidates === null && !error && <p className="hint">Reading ~/.ssh/config…</p>}
      {candidates?.length === 0 && <p className="hint">No concrete Host entries found in ~/.ssh/config.</p>}
      {candidates && candidates.length > 0 && (
        <>
          <p className="hint">
            Imported into the “SSH config” folder. Existing session names are skipped. Passwords are never imported.
          </p>
          <ul className="import-list">
            {candidates.map((c) => (
              <li key={c.alias}>
                <label className="checkbox">
                  <input type="checkbox" checked={selected.has(c.alias)} onChange={() => toggle(c.alias)} />
                  <span className="import-name">{c.alias}</span>
                  <span className="import-target">
                    {targetLabel(c.input)} · {c.input.auth.kind === "keyFile" ? "key" : c.input.auth.kind}
                    {c.jumpAlias && ` · via ${c.jumpAlias}`}
                  </span>
                </label>
                {c.warnings.map((w) => (
                  <p key={w} className="import-warning">⚠ {w}</p>
                ))}
              </li>
            ))}
          </ul>
        </>
      )}
    </Dialog>
  );
}
