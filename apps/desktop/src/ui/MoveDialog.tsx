import { useState } from "react";
import { Dialog } from "./Dialog";

interface Props {
  /** What is moved, e.g. `session "web-01"`. */
  what: string;
  /** Valid destinations; "" is the top level. */
  targets: string[];
  onMove: (target: string) => void;
  onCancel: () => void;
}

/** Keyboard-friendly alternative to dragging a session or folder. */
export function MoveDialog({ what, targets, onMove, onCancel }: Props) {
  const [target, setTarget] = useState(targets[0] ?? "");
  return (
    <Dialog
      title="Move to folder"
      onCancel={onCancel}
      actions={
        <>
          <button type="button" onClick={onCancel}>
            Cancel
          </button>
          <button type="button" className="primary" disabled={targets.length === 0} onClick={() => onMove(target)}>
            Move
          </button>
        </>
      }
    >
      {targets.length === 0 ? (
        <p>There is no other folder to move {what} to.</p>
      ) : (
        <div className="form-grid">
          <label className="span-2">
            Move {what} to
          <select value={target} onChange={(e) => setTarget(e.target.value)} autoFocus>
            {targets.map((t) => (
              <option key={t} value={t}>
                {t === "" ? "Top level" : t.split("/").join(" / ")}
              </option>
            ))}
          </select>
          </label>
        </div>
      )}
    </Dialog>
  );
}
