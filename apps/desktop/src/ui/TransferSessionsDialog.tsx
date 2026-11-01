import { useEffect, useState } from "react";
import { exportSessions, importSessions, isIpcError } from "../terminal/api";
import { Dialog } from "./Dialog";

interface Props {
  mode: "export" | "import";
  onDone: (summary: string) => void;
  onCancel: () => void;
}

/** Export (shows JSON, saves a copy in Downloads) or import (paste JSON). */
export function TransferSessionsDialog({ mode, onDone, onCancel }: Props) {
  const [text, setText] = useState("");
  const [savedTo, setSavedTo] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    if (mode !== "export") return;
    exportSessions().then(
      (r) => {
        setText(r.json);
        setSavedTo(r.savedTo);
      },
      (e) => setError(isIpcError(e) ? e.message : String(e)),
    );
  }, [mode]);

  async function runImport() {
    setError(null);
    try {
      const r = await importSessions(text);
      const parts = [`Imported ${r.created.length} session(s)`];
      if (r.skipped.length) parts.push(`skipped ${r.skipped.length} existing`);
      if (r.unresolvedJumps.length) parts.push(`jump host not found for ${r.unresolvedJumps.join(", ")}`);
      if (r.notImported.length)
        parts.push(`not imported: ${r.notImported.slice(0, 5).join("; ")}${r.notImported.length > 5 ? ` and ${r.notImported.length - 5} more` : ""}`);
      onDone(parts.join("; ") + ".");
    } catch (e) {
      setError(isIpcError(e) ? e.message : String(e));
    }
  }

  async function copy() {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
    } catch {
      setError("Copy failed; select the text and copy it manually.");
    }
  }

  return (
    <Dialog
      title={mode === "export" ? "Export sessions" : "Import sessions"}
      onCancel={onCancel}
      actions={
        mode === "export" ? (
          <>
            <button type="button" onClick={() => void copy()} disabled={!text}>
              {copied ? "Copied" : "Copy"}
            </button>
            <button type="button" className="primary" onClick={onCancel}>
              Done
            </button>
          </>
        ) : (
          <>
            <button type="button" onClick={onCancel}>
              Cancel
            </button>
            <button type="button" className="primary" disabled={!text.trim()} onClick={() => void runImport()}>
              Import
            </button>
          </>
        )
      }
    >
      <p className="hint">
        {mode === "export"
          ? "Sessions without passwords or keychain entries. Key files are referenced by path."
          : "Paste a Termalith session export, or the contents of a MobaXterm .mxtsessions file exported from MobaXterm (SSH, RDP and VNC sessions are imported). Sessions with existing names are skipped."}
      </p>
      {savedTo && (
        <p className="hint">
          Saved a copy to <code>{savedTo}</code>
        </p>
      )}
      <textarea
        className="json-box"
        value={text}
        readOnly={mode === "export"}
        spellCheck={false}
        placeholder={mode === "import" ? '{ "format": "termalith-sessions", … }  or  [Bookmarks] …' : ""}
        onChange={(e) => setText(e.target.value)}
        aria-label={mode === "export" ? "Exported sessions" : "Sessions to import"}
      />
      {error && <p className="form-error" role="alert">{error}</p>}
    </Dialog>
  );
}
