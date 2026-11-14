import { useEffect, useState } from "react";
import {
  type SharedSource,
  addSharedFile,
  addSharedUrl,
  isIpcError,
  listSharedSources,
  removeSharedSource,
  syncShared,
} from "../terminal/api";

const msg = (e: unknown) => (isIpcError(e) ? e.message : String(e));

/**
 * Settings section for shared (team) session files: a Termalith export on
 * a network share or an https:// address. Acts at once (not on Save).
 */
export function SharedSessionsSettings({ onChanged }: { onChanged: () => void }) {
  const [sources, setSources] = useState<SharedSource[]>([]);
  const [label, setLabel] = useState("");
  const [url, setUrl] = useState("");
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState<string | null>(null);

  const reload = () => listSharedSources().then(setSources, (e) => setNote(msg(e)));
  useEffect(() => {
    void reload();
  }, []);

  async function act(run: () => Promise<unknown>, sync = true) {
    setBusy(true);
    setNote(null);
    try {
      await run();
      if (sync) {
        const report = await syncShared();
        const failed = report.filter((r) => r.error);
        if (failed.length) setNote(failed.map((r) => `${r.label}: ${r.error}`).join("\n"));
      }
      await reload();
      onChanged();
    } catch (e) {
      setNote(msg(e));
    } finally {
      setBusy(false);
    }
  }

  const name = label.trim();
  return (
    <fieldset className="span-2 shared-sources">
      <legend>Shared sessions</legend>
      <p className="hint">
        Sessions from a Termalith session export on a shared folder or an https:// address appear under a folder
        with this name, read-only, and are refreshed at every start. Saved passwords stay on this computer.
      </p>
      {sources.length > 0 && (
        <ul className="shared-list">
          {sources.map((s) => (
            <li key={s.id}>
              <strong>{s.label}</strong> <code title={s.location}>{s.location}</code>{" "}
              <span className="hint">
                {s.lastError ? `could not read: ${s.lastError}` : `${s.sessions} session${s.sessions === 1 ? "" : "s"}`}
              </span>
              <button type="button" disabled={busy} onClick={() => void act(() => removeSharedSource(s.id), false)}>
                Remove
              </button>
            </li>
          ))}
        </ul>
      )}
      <div className="row">
        <input value={label} placeholder="Folder name, e.g. Team" aria-label="Shared folder name"
          onChange={(e) => setLabel(e.target.value)} />
        <button type="button" disabled={busy || !name} onClick={() => void act(async () => {
          const id = await addSharedFile(name);
          if (id) setLabel("");
        })}>
          Add file…
        </button>
      </div>
      <div className="row">
        <input value={url} placeholder="https://intranet.example/termalith-sessions.json" aria-label="Shared file address"
          spellCheck={false} onChange={(e) => setUrl(e.target.value)} />
        <button type="button" disabled={busy || !name || !url.trim().startsWith("https://")} onClick={() => void act(async () => {
          await addSharedUrl(name, url.trim());
          setLabel("");
          setUrl("");
        })}>
          Add address
        </button>
        {sources.length > 0 && (
          <button type="button" disabled={busy} onClick={() => void act(async () => {})}>
            Refresh now
          </button>
        )}
      </div>
      {note && <p className="files-error" role="alert" style={{ whiteSpace: "pre-line" }}>{note}</p>}
    </fieldset>
  );
}
