import { type FormEvent, useCallback, useEffect, useMemo, useState } from "react";
import {
  type Snippet,
  type SnippetInput,
  type TerminalId,
  createSnippet,
  deleteSnippet,
  isIpcError,
  listSnippets,
  updateSnippet,
  writeTerminal,
} from "../terminal/api";
import { fillSnippet, snippetVariables, terminalInput } from "./snippetText";

interface Props {
  terminalId: TerminalId;
  onClose: () => void;
}

type Mode = { kind: "list" } | { kind: "use"; snippet: Snippet } | { kind: "edit"; snippet: Snippet | null };

export function SnippetsPanel({ terminalId, onClose }: Props) {
  const [snippets, setSnippets] = useState<Snippet[]>([]);
  const [mode, setMode] = useState<Mode>({ kind: "list" });
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(() => {
    listSnippets().then(setSnippets, (e) => setError(isIpcError(e) ? e.message : String(e)));
  }, []);
  useEffect(refresh, [refresh]);

  return (
    <section className="files-panel" aria-label="Snippets">
      <header className="files-head">
        {mode.kind !== "list" && (
          <button type="button" className="icon" aria-label="Back" title="Back" onClick={() => setMode({ kind: "list" })}>
            ←
          </button>
        )}
        <strong className="panel-title">Snippets</strong>
        {mode.kind === "list" && (
          <button type="button" className="icon" aria-label="New snippet" title="New snippet"
            onClick={() => setMode({ kind: "edit", snippet: null })}>
            +
          </button>
        )}
        <button type="button" className="icon" aria-label="Close snippets" title="Close" onClick={onClose}>
          ×
        </button>
      </header>
      {error && <p className="files-error" role="alert">{error}</p>}

      {mode.kind === "list" && (
        <ul className="files-list">
          {snippets.length === 0 && <li className="files-empty">No snippets yet. Use + to save a command.</li>}
          {snippets.map((s, i) => (
            <li key={s.id}>
              {s.folder && s.folder !== snippets[i - 1]?.folder && <h4 className="snippet-folder">{s.folder}</h4>}
              <div className="file-row">
                <button type="button" className="snippet-name" onClick={() => setMode({ kind: "use", snippet: s })} title={s.command}>
                  {s.name}
                </button>
                <button type="button" className="icon" aria-label={`Edit ${s.name}`} title="Edit"
                  onClick={() => setMode({ kind: "edit", snippet: s })}>
                  ✎
                </button>
                <button type="button" className="icon danger" aria-label={`Delete ${s.name}`} title="Delete"
                  onClick={() => void deleteSnippet(s.id).then(refresh, (e) => setError(String(e?.message ?? e)))}>
                  ×
                </button>
              </div>
            </li>
          ))}
        </ul>
      )}

      {mode.kind === "use" && (
        <UseSnippet snippet={mode.snippet} terminalId={terminalId} onSent={() => setMode({ kind: "list" })} />
      )}

      {mode.kind === "edit" && (
        <EditSnippet
          snippet={mode.snippet}
          onSaved={() => {
            setMode({ kind: "list" });
            refresh();
          }}
        />
      )}
    </section>
  );
}

function UseSnippet({ snippet, terminalId, onSent }: { snippet: Snippet; terminalId: TerminalId; onSent: () => void }) {
  const vars = useMemo(() => snippetVariables(snippet.command), [snippet.command]);
  const [values, setValues] = useState<Record<string, string>>({});
  const [pressEnter, setPressEnter] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const filled = fillSnippet(snippet.command, values);
  const missing = vars.filter((v) => !(values[v] ?? "").length);

  async function send(e: FormEvent) {
    e.preventDefault();
    try {
      await writeTerminal(terminalId, terminalInput(filled, pressEnter));
      onSent();
    } catch (err) {
      setError(isIpcError(err) ? err.message : String(err));
    }
  }

  return (
    <form className="tunnel-form" onSubmit={send}>
      <strong>{snippet.name}</strong>
      {vars.map((v, i) => (
        <label key={v}>
          {v}
          <input autoFocus={i === 0} value={values[v] ?? ""} onChange={(e) => setValues((x) => ({ ...x, [v]: e.target.value }))} />
        </label>
      ))}
      <label>
        Will be typed into this terminal
        <pre className="snippet-preview">{filled}</pre>
      </label>
      <label className="checkbox">
        <input type="checkbox" checked={pressEnter} onChange={(e) => setPressEnter(e.target.checked)} />
        Press Enter (run it)
      </label>
      {error && <p className="form-error" role="alert">{error}</p>}
      <button type="submit" className="primary" disabled={missing.length > 0}>
        {missing.length ? `Fill in ${missing.join(", ")}` : pressEnter ? "Send and run" : "Send"}
      </button>
    </form>
  );
}

function EditSnippet({ snippet, onSaved }: { snippet: Snippet | null; onSaved: () => void }) {
  const [form, setForm] = useState<SnippetInput>(
    snippet ? { name: snippet.name, command: snippet.command, folder: snippet.folder } : { name: "", command: "", folder: null },
  );
  const [error, setError] = useState<string | null>(null);

  async function save(e: FormEvent) {
    e.preventDefault();
    const input = { ...form, folder: form.folder?.trim() || null };
    try {
      if (snippet) await updateSnippet(snippet.id, input);
      else await createSnippet(input);
      onSaved();
    } catch (err) {
      setError(isIpcError(err) ? err.message : String(err));
    }
  }

  return (
    <form className="tunnel-form" onSubmit={save}>
      <label>
        Name
        <input autoFocus value={form.name} onChange={(e) => setForm({ ...form, name: e.target.value })} />
      </label>
      <label>
        Command (use {"{{name}}"} for values asked when sending)
        <textarea
          className="json-box snippet-editor"
          value={form.command}
          spellCheck={false}
          onChange={(e) => setForm({ ...form, command: e.target.value })}
        />
      </label>
      <label>
        Folder
        <input value={form.folder ?? ""} placeholder="Optional" onChange={(e) => setForm({ ...form, folder: e.target.value })} />
      </label>
      {error && <p className="form-error" role="alert">{error}</p>}
      <button type="submit" className="primary">
        Save snippet
      </button>
    </form>
  );
}
