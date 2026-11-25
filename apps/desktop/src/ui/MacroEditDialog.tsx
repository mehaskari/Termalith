import { useState } from "react";
import { Plus, Trash2 } from "lucide-react";
import { type Macro, type MacroStep, isIpcError, updateMacro } from "../terminal/api";
import { decodeKeys, encodeKeys } from "../terminal/macro";
import { Dialog } from "./Dialog";

interface Row {
  id: number;
  wait: string;
  keys: string;
}

/** Rows to steps, or the number of the first row with a bad wait time. */
export function rowsToSteps(rows: Array<{ wait: string; keys: string }>): MacroStep[] | { badRow: number } {
  const steps: MacroStep[] = [];
  for (const [i, r] of rows.entries()) {
    const seconds = r.wait.trim() === "" ? 0 : Number(r.wait.replace(",", "."));
    if (!Number.isFinite(seconds) || seconds < 0 || seconds > 600) return { badRow: i + 1 };
    const data = decodeKeys(r.keys);
    if (data === "" && seconds === 0) continue;
    steps.push({ delayMs: Math.round(seconds * 1000), data });
  }
  return steps;
}

/**
 * Edits a recorded macro: its name, and each step's pause and keys as text
 * (Enter is \r, Escape \e, Tab \t). Macros are stored unencrypted.
 */
export function MacroEditDialog({ macro, onSaved, onCancel }: { macro: Macro; onSaved: () => void; onCancel: () => void }) {
  const [name, setName] = useState(macro.name);
  const [rows, setRows] = useState<Row[]>(() =>
    macro.steps.map((s, i) => ({ id: i, wait: s.delayMs ? String(s.delayMs / 1000) : "", keys: encodeKeys(s.data) })),
  );
  const [nextId, setNextId] = useState(macro.steps.length);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const patch = (id: number, p: Partial<Row>) => setRows((rs) => rs.map((r) => (r.id === id ? { ...r, ...p } : r)));

  async function save() {
    const steps = rowsToSteps(rows);
    if ("badRow" in steps) {
      setError(`Step ${steps.badRow}: the wait is a number of seconds (0 to 600).`);
      return;
    }
    if (steps.length === 0) {
      setError("A macro needs at least one step with keys.");
      return;
    }
    setBusy(true);
    try {
      await updateMacro(macro.id, { name, steps });
      onSaved();
    } catch (e) {
      setError(isIpcError(e) ? e.message : String(e));
      setBusy(false);
    }
  }

  return (
    <Dialog
      title="Edit macro"
      onCancel={onCancel}
      actions={
        <>
          <button type="button" onClick={onCancel}>
            Cancel
          </button>
          <button type="button" className="primary" disabled={busy || !name.trim()} onClick={() => void save()}>
            Save
          </button>
        </>
      }
    >
      <div className="form-grid">
        <label className="span-2">
          Name
          <input value={name} onChange={(e) => setName(e.target.value)} autoFocus />
        </label>
        <p className="hint span-2">
          Each step waits, then types its keys. Write Enter as <code>\r</code>, Tab as <code>\t</code>, Escape as{" "}
          <code>\e</code> and a backslash as <code>\\</code>.
        </p>
        <ol className="macro-steps span-2">
          {rows.map((r, i) => (
            <li key={r.id}>
              <input
                className="macro-wait"
                inputMode="decimal"
                value={r.wait}
                placeholder="0 s"
                aria-label={`Step ${i + 1} wait in seconds`}
                onChange={(e) => patch(r.id, { wait: e.target.value })}
              />
              <input
                className="macro-keys-input"
                value={r.keys}
                spellCheck={false}
                autoCapitalize="off"
                aria-label={`Step ${i + 1} keys`}
                onChange={(e) => patch(r.id, { keys: e.target.value })}
              />
              <button
                type="button"
                className="icon danger"
                aria-label={`Delete step ${i + 1}`}
                title="Delete step"
                onClick={() => setRows((rs) => rs.filter((x) => x.id !== r.id))}
              >
                <Trash2 size={14} aria-hidden="true" />
              </button>
            </li>
          ))}
        </ol>
        <div className="span-2">
          <button
            type="button"
            onClick={() => {
              setRows((rs) => [...rs, { id: nextId, wait: "", keys: "" }]);
              setNextId((n) => n + 1);
            }}
          >
            <Plus size={14} aria-hidden="true" />
            Add step
          </button>
        </div>
        <p className="pane-warning span-2">Macros are stored unencrypted: do not put passwords in them.</p>
        {error && (
          <p className="form-error span-2" role="alert">
            {error}
          </p>
        )}
      </div>
    </Dialog>
  );
}
