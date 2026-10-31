import { useState } from "react";
import type { BroadcastTab } from "./broadcast";
import { Dialog } from "./Dialog";

interface Props {
  source: BroadcastTab;
  candidates: BroadcastTab[];
  onStart: (targets: number[]) => void;
  onCancel: () => void;
}

/** Explicit, previewed opt-in: nothing is mirrored until the user confirms targets. */
export function BroadcastDialog({ source, candidates, onStart, onCancel }: Props) {
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const toggle = (k: number) =>
    setSelected((s) => {
      const n = new Set(s);
      if (n.has(k)) n.delete(k);
      else n.add(k);
      return n;
    });

  return (
    <Dialog
      title="Broadcast input"
      tone="warning"
      onCancel={onCancel}
      actions={
        <>
          <button type="button" onClick={onCancel}>
            Cancel
          </button>
          <button type="button" className="danger" disabled={selected.size === 0} onClick={() => onStart([...selected])}>
            Broadcast to {selected.size} terminal{selected.size === 1 ? "" : "s"}
          </button>
        </>
      }
    >
      <p>
        Everything you type in <strong>{source.title}</strong> will also be typed into the terminals you select,
        including Enter. Commands run on every selected machine.
      </p>
      {candidates.length === 0 ? (
        <p className="hint">No other connected terminals are open.</p>
      ) : (
        <ul className="import-list">
          {candidates.map((t) => (
            <li key={t.key}>
              <label className="checkbox">
                <input type="checkbox" checked={selected.has(t.key)} onChange={() => toggle(t.key)} />
                <span className="import-name">{t.title}</span>
                <span className="import-target">{t.kind}</span>
              </label>
            </li>
          ))}
        </ul>
      )}
    </Dialog>
  );
}
