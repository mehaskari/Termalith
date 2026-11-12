import { useState } from "react";
import type { BroadcastTab } from "./broadcast";
import { Dialog } from "./Dialog";

interface Props {
  source: BroadcastTab;
  candidates: BroadcastTab[];
  /** Ticked at first (a folder opened for broadcasting). */
  preselected?: number[];
  onStart: (targets: number[], tiled: boolean) => void;
  onCancel: () => void;
}

/** Explicit, previewed opt-in: nothing is mirrored until the user confirms targets. */
export function BroadcastDialog({ source, candidates, preselected = [], onStart, onCancel }: Props) {
  const [selected, setSelected] = useState<Set<number>>(
    () => new Set(preselected.filter((k) => candidates.some((c) => c.key === k))),
  );
  const [tiled, setTiled] = useState(preselected.length > 0);
  const allSelected = candidates.length > 0 && selected.size === candidates.length;
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
          <button type="button" className="danger" disabled={selected.size === 0} onClick={() => onStart([...selected], tiled)}>
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
        <>
        <div className="row">
          <button
            type="button"
            className="link"
            onClick={() => setSelected(allSelected ? new Set() : new Set(candidates.map((c) => c.key)))}
          >
            {allSelected ? "Select none" : "Select all"}
          </button>
        </div>
        <ul className="import-list">
          {candidates.map((t) => (
            <li key={t.key}>
              <label className="checkbox">
                <input type="checkbox" checked={selected.has(t.key)} onChange={() => toggle(t.key)} />
                <span className="import-name">{t.title}</span>
                <span className="import-target">{t.terminalId === undefined ? "connecting…" : t.kind}</span>
              </label>
            </li>
          ))}
        </ul>
        <label className="checkbox">
          <input type="checkbox" checked={tiled} onChange={(e) => setTiled(e.target.checked)} />
          Show the terminals side by side
        </label>
        </>
      )}
    </Dialog>
  );
}
