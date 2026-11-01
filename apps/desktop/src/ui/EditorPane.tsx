import { type KeyboardEvent, useCallback, useEffect, useRef, useState } from "react";
import { type EditVersion, type TerminalId, isIpcError, readRemoteText, saveRemoteText } from "../terminal/api";

interface Props {
  terminalId: TerminalId;
  dir: string;
  name: string;
  active: boolean;
  /** Reports unsaved changes so closing the tab can ask first. */
  onDirty: (dirty: boolean) => void;
}

type Status =
  | { kind: "loading" }
  | { kind: "ready" }
  | { kind: "saving" }
  | { kind: "saved" }
  | { kind: "error"; message: string }
  | { kind: "conflict" };

/** Plain-text editor for one remote file, saved back over SFTP. */
export function EditorPane({ terminalId, dir, name, active, onDirty }: Props) {
  const [text, setText] = useState("");
  const [saved, setSaved] = useState<string | null>(null);
  const [version, setVersion] = useState<EditVersion | null>(null);
  const [path, setPath] = useState(`${dir}/${name}`);
  const [status, setStatus] = useState<Status>({ kind: "loading" });
  const area = useRef<HTMLTextAreaElement>(null);
  const dirty = saved !== null && text !== saved;

  useEffect(() => onDirty(dirty), [dirty, onDirty]);

  const load = useCallback(async () => {
    setStatus({ kind: "loading" });
    try {
      const r = await readRemoteText(terminalId, dir, name);
      setText(r.content);
      setSaved(r.content);
      setVersion(r.version);
      setPath(r.path);
      setStatus({ kind: "ready" });
    } catch (e) {
      setStatus({ kind: "error", message: isIpcError(e) ? e.message : String(e) });
    }
  }, [terminalId, dir, name]);

  useEffect(() => {
    void load();
  }, [load]);

  useEffect(() => {
    if (active && status.kind === "ready") area.current?.focus();
  }, [active, status.kind]);

  async function save(force = false) {
    if (!version) return;
    setStatus({ kind: "saving" });
    try {
      // Overwriting someone else's change: take the current version first.
      const base = force ? (await readRemoteText(terminalId, dir, name)).version : version;
      const next = await saveRemoteText(terminalId, dir, name, text, base);
      setVersion(next);
      setSaved(text);
      setStatus({ kind: "saved" });
    } catch (e) {
      if (isIpcError(e) && e.code === "remote_modified") setStatus({ kind: "conflict" });
      else setStatus({ kind: "error", message: isIpcError(e) ? e.message : String(e) });
    }
  }

  function onKeyDown(e: KeyboardEvent<HTMLTextAreaElement>) {
    if ((e.metaKey || e.ctrlKey) && !e.shiftKey && !e.altKey && e.key.toLowerCase() === "s") {
      e.preventDefault();
      if (dirty) void save();
    } else if (e.key === "Tab" && !e.metaKey && !e.ctrlKey && !e.altKey) {
      // Insert a tab instead of moving focus; Esc then Tab still leaves.
      e.preventDefault();
      const el = e.currentTarget;
      const { selectionStart: a, selectionEnd: b } = el;
      setText(text.slice(0, a) + "\t" + text.slice(b));
      requestAnimationFrame(() => el.setSelectionRange(a + 1, a + 1));
    }
  }

  const loaded = saved !== null;
  return (
    <div className="editor-pane">
      <div className="editor-bar">
        <code className="editor-path" title={path}>
          {path}
          {dirty && " •"}
        </code>
        <span className="editor-status" role="status">
          {status.kind === "loading" && "Loading…"}
          {status.kind === "saving" && "Saving…"}
          {status.kind === "saved" && !dirty && "Saved"}
          {status.kind === "error" && <span className="form-error">{status.message}</span>}
          {status.kind === "conflict" && (
            <span className="pane-warning">
              The file changed on the server since you opened it.
              <button type="button" onClick={() => void load()}>
                Reload (discard mine)
              </button>
              <button type="button" className="danger" onClick={() => void save(true)}>
                Overwrite
              </button>
            </span>
          )}
        </span>
        <span className="pane-actions">
          <button type="button" disabled={!loaded || status.kind === "saving"} onClick={() => void load()}>
            Reload
          </button>
          <button type="button" className="primary" disabled={!dirty || status.kind === "saving"} onClick={() => void save()}>
            Save
          </button>
        </span>
      </div>
      <textarea
        ref={area}
        className="editor-text"
        value={text}
        disabled={!loaded}
        spellCheck={false}
        autoCapitalize="off"
        autoCorrect="off"
        wrap="off"
        aria-label={`Contents of ${name}`}
        onChange={(e) => setText(e.target.value)}
        onKeyDown={onKeyDown}
      />
    </div>
  );
}
