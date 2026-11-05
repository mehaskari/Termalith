import { useState } from "react";
import type { MacroStep } from "../terminal/api";
import { describeKeys } from "../terminal/macro";
import { Dialog } from "./Dialog";

interface Props {
  steps: MacroStep[];
  onSave: (name: string) => void;
  onDiscard: () => void;
}

/** Shows what was recorded before anything is stored. */
export function MacroSaveDialog({ steps, onSave, onDiscard }: Props) {
  const [name, setName] = useState("");
  return (
    <Dialog
      title="Save macro"
      onCancel={onDiscard}
      actions={
        <>
          <button type="button" onClick={onDiscard}>
            Discard
          </button>
          <button type="button" className="primary" disabled={!name.trim() || steps.length === 0} onClick={() => onSave(name)}>
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
        <p className="pane-warning span-2">
          Check the keys below. If you typed a password while recording, it is here: discard the macro. Macros are
          stored unencrypted.
        </p>
        <pre className="macro-keys span-2">{describeKeys(steps) || "Nothing was typed."}</pre>
      </div>
    </Dialog>
  );
}
