import { useEffect, useState } from "react";
import { type TransferRecord, clearTransferHistory, isIpcError, transferHistory } from "../terminal/api";
import { formatSize } from "./paths";

const ICON = { download: "↓", upload: "↑" };
const OUTCOME = { done: "done", failed: "failed", cancelled: "cancelled" };

/** Finished transfers (all connections), most recent first. */
export function TransferHistory() {
  const [items, setItems] = useState<TransferRecord[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const load = () => transferHistory(200).then(setItems, (e) => setError(isIpcError(e) ? e.message : String(e)));
  useEffect(() => {
    void load();
  }, []);
  return (
    <div className="history">
      <div className="history-head">
        <span>Recent transfers</span>
        <button
          type="button"
          className="link"
          disabled={!items?.length}
          onClick={() => clearTransferHistory().then(load, () => {})}
        >
          Clear
        </button>
      </div>
      {error && <p className="files-error">{error}</p>}
      {items?.length === 0 && <p className="files-empty">No transfers yet.</p>}
      <ul className="history-list">
        {items?.map((t) => (
          <li key={t.id} className={`history-item ${t.outcome}`} title={t.message ?? t.localPath ?? t.remotePath}>
            <span className="history-dir" aria-label={t.direction}>
              {ICON[t.direction]}
            </span>
            <span className="history-name">
              {t.name}
              <small>
                {t.connection ? `${t.connection} · ` : ""}
                {new Date(t.finishedAt * 1000).toLocaleString()}
                {t.size !== null ? ` · ${formatSize(t.size)}` : ""}
              </small>
            </span>
            <span className={`history-outcome ${t.outcome}`}>{OUTCOME[t.outcome]}</span>
          </li>
        ))}
      </ul>
    </div>
  );
}
