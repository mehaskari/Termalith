import { useCallback, useEffect, useState } from "react";
import {
  type DroppedFile,
  type RemoteEntry,
  type TerminalId,
  type TransferEvent,
  cancelTransfer,
  isIpcError,
  sftpDownload,
  sftpHome,
  sftpList,
  sftpUpload,
} from "../terminal/api";
import { formatSize, joinRemote, parentRemote, percent } from "./paths";

interface Transfer {
  id: number | null;
  key: number;
  direction: "down" | "up";
  name: string;
  transferred: number;
  total: number | null;
  state: "running" | "done" | "failed" | "cancelled";
  detail?: string;
}

interface Props {
  terminalId: TerminalId;
  /** Files dropped on the window while this panel is visible. */
  dropped: DroppedFile[] | null;
  onDroppedHandled: () => void;
  onClose: () => void;
}

let transferKey = 1;

function message(e: unknown): string {
  return isIpcError(e) ? e.message : String(e);
}

export function FilesPanel({ terminalId, dropped, onDroppedHandled, onClose }: Props) {
  const [path, setPath] = useState<string | null>(null);
  const [pathInput, setPathInput] = useState("");
  const [entries, setEntries] = useState<RemoteEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [transfers, setTransfers] = useState<Transfer[]>([]);

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
      case "progress":
        update(key, { transferred: e.transferred, total: e.total });
        break;
      case "done":
        update(key, { state: "done", detail: e.destination });
        if (refreshAfter && path) void load(path);
        break;
      case "failed":
        update(key, { state: "failed", detail: e.message });
        break;
      case "cancelled":
        update(key, { state: "cancelled" });
        break;
    }
  };

  function download(entry: RemoteEntry) {
    if (!path) return;
    const key = transferKey++;
    setTransfers((ts) => [
      { id: null, key, direction: "down", name: entry.name, transferred: 0, total: entry.size, state: "running" },
      ...ts,
    ]);
    sftpDownload(terminalId, path, entry.name, entry.size, onEvent(key, false)).then(
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
      sftpUpload(terminalId, f.token, path, onEvent(key, true)).then(
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
        <button type="button" className="icon" title="Refresh" aria-label="Refresh" disabled={!path}
          onClick={() => path && void load(path)}>
          ⟳
        </button>
        <button type="button" className="icon" title="Close files" aria-label="Close files" onClick={onClose}>
          ×
        </button>
      </header>

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
                  <button type="button" className="icon" title={`Download ${e.name}`} aria-label={`Download ${e.name}`}
                    onClick={() => download(e)}>
                    ↓
                  </button>
                )}
              </div>
            )}
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
                  {t.state === "running" && t.id !== null && (
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
                    {t.state === "done" ? (t.direction === "down" ? `Saved to ${t.detail}` : "Uploaded") : t.state === "cancelled" ? "Cancelled" : t.detail}
                  </span>
                )}
              </li>
            );
          })}
        </ul>
      )}
    </section>
  );
}
