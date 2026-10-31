import { type FormEvent, useState } from "react";
import { type Settings, isIpcError } from "../terminal/api";
import { Dialog } from "./Dialog";

interface Props {
  initial: Settings;
  onSave: (s: Settings) => Promise<void>;
  onCancel: () => void;
}

export function SettingsDialog({ initial, onSave, onCancel }: Props) {
  const [s, setS] = useState<Settings>(initial);
  const [error, setError] = useState<string | null>(null);
  const set = (patch: Partial<Settings>) => setS((x) => ({ ...x, ...patch }));

  async function submit(e: FormEvent) {
    e.preventDefault();
    if (!Number.isInteger(s.fontSize) || s.fontSize < 8 || s.fontSize > 32) return setError("Font size must be 8-32");
    if (!Number.isInteger(s.scrollback) || s.scrollback < 100 || s.scrollback > 100000)
      return setError("Scrollback must be 100-100000 lines");
    try {
      await onSave(s);
    } catch (err) {
      setError(isIpcError(err) ? err.message : String(err));
    }
  }

  return (
    <Dialog
      title="Settings"
      onCancel={onCancel}
      actions={
        <>
          <button type="button" onClick={onCancel}>
            Cancel
          </button>
          <button type="submit" form="settings-form" className="primary">
            Save
          </button>
        </>
      }
    >
      <form id="settings-form" className="form-grid" onSubmit={submit}>
        <label>
          Theme
          <select value={s.theme} onChange={(e) => set({ theme: e.target.value as Settings["theme"] })}>
            <option value="system">Match system</option>
            <option value="dark">Dark</option>
            <option value="light">Light</option>
          </select>
        </label>
        <label>
          Font size
          <input type="number" min={8} max={32} value={s.fontSize} onChange={(e) => set({ fontSize: Number(e.target.value) })} />
        </label>
        <label className="span-2">
          Terminal font
          <input
            value={s.fontFamily}
            placeholder="Default (Menlo / Consolas / DejaVu Sans Mono)"
            onChange={(e) => set({ fontFamily: e.target.value })}
          />
        </label>
        <label>
          Cursor
          <select value={s.cursorStyle} onChange={(e) => set({ cursorStyle: e.target.value as Settings["cursorStyle"] })}>
            <option value="block">Block</option>
            <option value="bar">Bar</option>
            <option value="underline">Underline</option>
          </select>
        </label>
        <label>
          Scrollback (lines)
          <input type="number" min={100} max={100000} value={s.scrollback} onChange={(e) => set({ scrollback: Number(e.target.value) })} />
        </label>
        <label className="checkbox span-2">
          <input type="checkbox" checked={s.cursorBlink} onChange={(e) => set({ cursorBlink: e.target.checked })} />
          Blinking cursor
        </label>
        {error && <p className="form-error span-2" role="alert">{error}</p>}
      </form>
    </Dialog>
  );
}
