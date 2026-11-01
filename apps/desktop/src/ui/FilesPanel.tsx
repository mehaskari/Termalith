import { useCallback, useEffect, useState } from "react";
import {
  type DroppedFile,
  type RemoteEntry,
  type TerminalId,
  type TransferEvent,
  cancelTransfer,
  isIpcError,
  retryTransfer,
  sftpDelete,
  sftpDownload,
  sftpMkdir,
  sftpRename,
  sftpHome,
  sftpList,
  sftpUpload,
} from "../terminal/api";
import { Dialog } from "./Dialog";
import { TransferHistory } from "./TransferHistory";
import { formatSize, joinRemote, parentRemote, percent } from "./paths";

type FileOp =
  | { kind: "mkdir" }
  | { kind: "rename"; entry: RemoteEntry }
  | { kind: "delete"; entry: RemoteEntry };

/** Asks for a file or folder name (validated again by the backend). */
function NameDialog(props: { title: string; initial: string; action: string; onSubmit: (name: string) => Promise<void>; onCancel: () => void }) {
  const [name, setName] = useState(props.initial);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const trimmed = name.trim();
  const invalid = !trimmed || trimmed === "." || trimmed === ".." || /[/\\]/.test(trimmed);
  return (
    <Dialog
      title={props.title}
      onCancel={props.onCancel}
      actions={
        <>
          <button type="button" onClick={props.onCancel}>
            Cancel
          </button>
          <button type="submit" form="name-form" className="primary" disabled={invalid || busy}>
            {props.action}
          </button>
        </>
      }
    >
      <form
        id="name-form"
        className="stack"
        onSubmit={(e) => {
          e.preventDefault();
          if (invalid) return;
          setBusy(true);
          props.onSubmit(trimmed).catch((err) => {
            setError(message(err));
            setBusy(false);
          });
        }}
      >
        <input
          autoFocus
          value={name}
          spellCheck={false}
          autoCapitalize="off"
          aria-label="Name"
          onChange={(e) => setName(e.target.value)}
          onFocus={(e) => {
            // Select the name without its extension, like file managers do.
            const dot = e.target.value.lastIndexOf(".");
            e.target.setSelectionRange(0, dot > 0 ? dot : e.target.value.length);
          }}
        />
        {invalid && trimmed && <p className="hint">A name cannot contain / or \\.</p>}
        {error && (
          <p className="form-error" role="alert">
            {error}
          </p>
        )}
      </form>
    </Dialog>
  );
}

interface Transfer {
  id: number | null;
  key: number;
  direction: "down" | "up";
  name: string;
  transferred: number;
  total: number | null;
  state: "queued" | "running" | "retrying" | "done" | "failed" | "cancelled";
  detail?: string;
  /** The backend can run it again (failed transiently, or cancelled). */
  retryable?: boolean;
}

interface Props {
  terminalId: TerminalId;
  /** Files dropped on the window while this panel is visible. */
  dropped: DroppedFile[] | null;
  onDroppedHandled: () => void;
  onClose: () => void;
  /** Open a remote file in the built-in editor. */
  onEdit: (dir: string, name: string) => void;
  /** Compare two remote files (the first was picked with "Compare…"). */
  onCompare?: (a: { dir: string; name: string }, b: { dir: string; name: string }) => void;
  /** Connection label recorded in the transfer history. */
  connection?: string;
}

let transferKey = 1;

function message(e: unknown): string {
  return isIpcError(e) ? e.message : String(e);
}

export function FilesPanel({ terminalId, dropped, onDroppedHandled, onClose, onEdit, onCompare, connection }: Props) {
  const [showHistory, setShowHistory] = useState(false);
  const [compareBase, setCompareBase] = useState<{ dir: string; name: string } | null>(null);
  const [path, setPath] = useState<string | null>(null);
  const [pathInput, setPathInput] = useState("");
  const [entries, setEntries] = useState<RemoteEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [transfers, setTransfers] = useState<Transfer[]>([]);
  const [op, setOp] = useState<FileOp | null>(null);
  const [menuFor, setMenuFor] = useState<string | null>(null);

  useEffect(() => {
    if (menuFor === null) return;
    const close = (e: MouseEvent) => {
      if (!(e.target as Element).closest?.(".file-menu")) setMenuFor(null);
    };
    window.addEventListener("mousedown", close);
    return () => window.removeEventListener("mousedown", close);
  }, [menuFor]);

  async function runOp(f: () => Promise<void>) {
    await f();
    setOp(null);
    if (path) await load(path);
  }

  const load = useCallback(
    async (p: string) => {
      setLoading(true);
      setError(null);
      try {
        setEntries(await sftpList(terminalId, p));
        setPath(p);
        setPathInput(p);
      } catch (e) {
        setError(message(e));
      } finally {
        setLoading(false);
      }
    },
    [terminalId],
  );

  useEffect(() => {
    sftpHome(terminalId).then(load, (e) => setError(message(e)));
  }, [terminalId, load]);

  const update = (key: number, patch: Partial<Transfer>) =>
    setTransfers((ts) => ts.map((t) => (t.key === key ? { ...t, ...patch } : t)));

  const onEvent = (key: number, refreshAfter: boolean) => (e: TransferEvent) => {
    switch (e.kind) {
      case "queued":
        update(key, { state: "queued" });
        break;
      case "progress":
        update(key, { state: "running", transferred: e.transferred, total: e.total });
        break;
      case "retrying":
        update(key, { state: "retrying", transferred: 0, detail: `${e.message} (retry ${e.attempt} of ${e.max})` });
        break;
      case "done":
        update(key, { state: "done", detail: e.destination });
        if (refreshAfter && path) void load(path);
        break;
      case "failed":
        update(key, { state: "failed", detail: e.message, retryable: e.retryable });
        break;
      case "cancelled":
        update(key, { state: "cancelled", retryable: true });
        break;
    }
  };

  function retry(t: Transfer) {
    if (t.id === null) return;
    update(t.key, { state: "running", transferred: 0, detail: undefined, retryable: false });
    retryTransfer(t.id, onEvent(t.key, t.direction === "up")).then(
      (id) => update(t.key, { id }),
      (e) => update(t.key, { state: "failed", detail: message(e), retryable: false }),
    );
  }

  function download(entry: RemoteEntry) {
    if (!path) return;
    const key = transferKey++;
    setTransfers((ts) => [
      { id: null, key, direction: "down", name: entry.name, transferred: 0, total: entry.size, state: "running" },
      ...ts,
    ]);
    sftpDownload(terminalId, path, entry.name, entry.size, onEvent(key, false), connection).then(
      (r) => update(key, { id: r.id }),
      (e) => update(key, { state: "failed", detail: message(e) }),
    );
  }

  // Upload files dropped onto the window into the current directory.
  useEffect(() => {
    if (!dropped || !path) return;
    for (const f of dropped) {
      const key = transferKey++;
      setTransfers((ts) => [
        { id: null, key, direction: "up", name: f.name, transferred: 0, total: f.size, state: "running" },
        ...ts,
      ]);
      sftpUpload(terminalId, f.token, path, onEvent(key, true), connection).then(
        (id) => update(key, { id }),
        (e) => update(key, { state: "failed", detail: message(e) }),
      );
    }
    onDroppedHandled();
    // onEvent/load capture `path`; re-running on their identity is unwanted.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [dropped]);

  return (
    <section className="files-panel" aria-label="Remote files">
      <header className="files-head">
        <button type="button" className="icon" title="Parent folder" aria-label="Parent folder"
          disabled={!path || parentRemote(path) === path} onClick={() => path && void load(parentRemote(path))}>
          ↑
        </button>
        <form
          className="files-path"
          onSubmit={(e) => {
            e.preventDefault();
            if (pathInput.trim()) void load(pathInput.trim());
          }}
        >
          <input value={pathInput} onChange={(e) => setPathInput(e.target.value)} aria-label="Remote path" spellCheck={false} />
        </form>
        <button type="button" className="icon" title="Transfer history" aria-label="Transfer history" aria-pressed={showHistory}
          onClick={() => setShowHistory((v) => !v)}>
          ⏱
        </button>
        <button type="button" className="icon" title="New folder" aria-label="New folder" disabled={!path}
          onClick={() => setOp({ kind: "mkdir" })}>
          +
        </button>
        <button type="button" className="icon" title="Refresh" aria-label="Refresh" disabled={!path}
          onClick={() => path && void load(path)}>
          ⟳
        </button>
        <button type="button" className="icon" title="Close files" aria-label="Close files" onClick={onClose}>
          ×
        </button>
      </header>

      {compareBase && (
        <p className="files-compare">
          Comparing <code>{compareBase.name}</code>: choose “Compare with …” on another file.
          <button type="button" className="link" onClick={() => setCompareBase(null)}>Cancel</button>
        </p>
      )}
      {showHistory && <TransferHistory />}
      {error && <p className="files-error" role="alert">{error}</p>}
      <ul className="files-list" aria-busy={loading}>
        {entries.map((e) => (
          <li key={e.name}>
            {e.kind === "dir" ? (
              <button type="button" className="file-row dir" onClick={() => path && void load(joinRemote(path, e.name))}>
                <span className="file-icon" aria-hidden="true">▸</span>
                <span className="file-name">{e.name}</span>
              </button>
            ) : (
              <div className="file-row">
                <span className="file-icon" aria-hidden="true">{e.kind === "symlink" ? "↪" : "·"}</span>
                <span className="file-name" title={e.name}>{e.name}</span>
                <span className="file-size">{formatSize(e.size)}</span>
                {e.kind === "file" && (
                  <button type="button" className="icon" title={`Edit ${e.name}`} aria-label={`Edit ${e.name}`}
                    onClick={() => path && onEdit(path, e.name)}>
                    ✎
                  </button>
                )}
                {e.kind === "file" && (
                  <button type="button" className="icon" title={`Download ${e.name}`} aria-label={`Download ${e.name}`}
                    onClick={() => download(e)}>
                    ↓
                  </button>
                )}
              </div>
            )}
            <span className="file-menu">
              <button type="button" className="icon" aria-label={`More actions for ${e.name}`} title="More"
                aria-haspopup="menu" aria-expanded={menuFor === e.name}
                onClick={() => setMenuFor(menuFor === e.name ? null : e.name)}>
                ⋯
              </button>
              {menuFor === e.name && (
                <span className="menu" role="menu">
                  <button type="button" role="menuitem" onClick={() => { setMenuFor(null); setOp({ kind: "rename", entry: e }); }}>
                    Rename…
                  </button>
                  {onCompare && e.kind === "file" && path && (
                    compareBase && !(compareBase.dir === path && compareBase.name === e.name) ? (
                      <button type="button" role="menuitem" onClick={() => {
                        setMenuFor(null);
                        onCompare(compareBase, { dir: path, name: e.name });
                        setCompareBase(null);
                      }}>
                        Compare with {compareBase.name}
                      </button>
                    ) : (
                      <button type="button" role="menuitem" onClick={() => { setMenuFor(null); setCompareBase({ dir: path, name: e.name }); }}>
                        Compare…
                      </button>
                    )
                  )}
                  <button type="button" role="menuitem" className="danger" onClick={() => { setMenuFor(null); setOp({ kind: "delete", entry: e }); }}>
                    Delete…
                  </button>
                </span>
              )}
            </span>
          </li>
        ))}
        {!loading && path && entries.length === 0 && !error && <li className="files-empty">Empty folder</li>}
      </ul>

      <p className="files-hint">Drop files onto the window to upload them here. Downloads go to your Downloads folder.</p>

      {transfers.length > 0 && (
        <ul className="transfers" aria-label="Transfers">
          {transfers.map((t) => {
            const pct = percent(t.transferred, t.total);
            return (
              <li key={t.key} className={`transfer ${t.state}`}>
                <div className="transfer-line">
                  <span aria-hidden="true">{t.direction === "down" ? "↓" : "↑"}</span>
                  <span className="file-name" title={t.name}>{t.name}</span>
                  {t.retryable && t.id !== null && (t.state === "failed" || t.state === "cancelled") && (
                    <button type="button" className="icon" aria-label={`Retry ${t.name}`} title="Retry"
                      onClick={() => retry(t)}>
                      ↻
                    </button>
                  )}
                  {(t.state === "running" || t.state === "queued" || t.state === "retrying") && t.id !== null && (
                    <button type="button" className="icon" aria-label={`Cancel ${t.name}`} title="Cancel"
                      onClick={() => t.id !== null && void cancelTransfer(t.id)}>
                      ×
                    </button>
                  )}
                </div>
                {t.state === "running" && (
                  <progress max={100} value={pct ?? undefined} aria-label={`${t.name} progress`} />
                )}
                {t.state !== "running" && (
                  <span className="transfer-detail" title={t.detail}>
                    {t.state === "done"
                      ? t.direction === "down"
                        ? `Saved to ${t.detail}`
                        : "Uploaded"
                      : t.state === "cancelled"
                        ? "Cancelled"
                        : t.state === "queued"
                          ? "Waiting for a free slot…"
                          : t.detail}
                  </span>
                )}
              </li>
            );
          })}
        </ul>
      )}
      {op?.kind === "mkdir" && path && (
        <NameDialog
          title="New folder"
          initial=""
          action="Create"
          onCancel={() => setOp(null)}
          onSubmit={(name) => runOp(() => sftpMkdir(terminalId, path, name))}
        />
      )}
      {op?.kind === "rename" && path && (
        <NameDialog
          title={`Rename ${op.entry.name}`}
          initial={op.entry.name}
          action="Rename"
          onCancel={() => setOp(null)}
          onSubmit={(name) =>
            name === op.entry.name ? Promise.resolve(setOp(null)) : runOp(() => sftpRename(terminalId, path, op.entry.name, name))
          }
        />
      )}
      {op?.kind === "delete" && path && (
        <Dialog
          title={`Delete ${op.entry.name}?`}
          tone="warning"
          onCancel={() => setOp(null)}
          actions={
            <>
              <button type="button" onClick={() => setOp(null)}>
                Cancel
              </button>
              <button
                type="button"
                className="danger"
                onClick={() => runOp(() => sftpDelete(terminalId, path, op.entry.name)).catch((e) => {
                  setOp(null);
                  setError(message(e));
                })}
              >
                Delete
              </button>
            </>
          }
        >
          <p>
            {op.entry.kind === "dir"
              ? "The folder is deleted only if it is empty."
              : "The file is deleted on the server. This cannot be undone."}
          </p>
        </Dialog>
      )}
    </section>
  );
}
