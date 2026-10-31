import { useMemo, useState } from "react";
import type { SessionRecord } from "../terminal/api";
import { authLabel, filterSessions, groupSessions, targetLabel } from "./sessionList";

interface Props {
  sessions: SessionRecord[];
  onConnect: (s: SessionRecord) => void;
  onEdit: (s: SessionRecord) => void;
  onDelete: (s: SessionRecord) => void;
  onForgetSecret: (s: SessionRecord) => void;
  onNew: () => void;
  onQuickConnect: () => void;
  onLocalTerminal: () => void;
  onImport: () => void;
  onToggleFavorite: (s: SessionRecord) => void;
  onDuplicate: (s: SessionRecord) => void;
  onExport: () => void;
  onImportSessions: () => void;
  onSettings: () => void;
}

export function Sidebar(p: Props) {
  const [query, setQuery] = useState("");
  const filtered = useMemo(() => filterSessions(p.sessions, query), [p.sessions, query]);
  const groups = useMemo(() => {
    const g = groupSessions(filtered);
    const favorites = filtered.filter((s) => s.favorite);
    return favorites.length ? [{ folder: "★ Favorites", sessions: favorites }, ...g] : g;
  }, [filtered]);

  return (
    <aside className="sidebar" aria-label="Sessions">
      <div className="sidebar-actions">
        <button type="button" className="primary" onClick={p.onQuickConnect}>
          Quick connect
        </button>
        <button type="button" onClick={p.onLocalTerminal} title="Open a local terminal">
          Local
        </button>
      </div>
      <div className="sidebar-head">
        <h2>Sessions</h2>
        <button type="button" className="icon" onClick={p.onNew} aria-label="New session" title="New session">
          +
        </button>
      </div>
      <input
        className="search"
        type="search"
        placeholder="Search"
        value={query}
        onChange={(e) => setQuery(e.target.value)}
        aria-label="Search sessions"
      />
      <nav className="session-list">
        {p.sessions.length === 0 && (
          <p className="empty">
            No saved sessions yet.
            <br />
            <button type="button" className="link" onClick={p.onNew}>
              Create one
            </button>
          </p>
        )}
        {p.sessions.length > 0 && groups.length === 0 && <p className="empty">No matches.</p>}
        {groups.map((g) => (
          <section key={`${g.folder ?? "\u0000"}`}>
            {g.folder ? <h3>{g.folder}</h3> : groups.length > 1 && <h3>Ungrouped</h3>}
            <ul>
              {g.sessions.map((s) => (
                <li key={s.id}>
                  <button
                    type="button"
                    className="session"
                    onClick={() => p.onConnect(s)}
                    title={`Connect to ${targetLabel(s)}`}
                  >
                    <span className="session-name">{s.name}</span>
                    <span className="session-target">
                      {targetLabel(s)}
                      {authLabel(s) && ` · ${authLabel(s)}`}
                      {s.jumpVia && ` · via ${p.sessions.find((j) => j.id === s.jumpVia)?.name ?? "?"}`}
                    </span>
                  </button>
                  <span className="row-actions">
                    <button
                      type="button"
                      className={`icon ${s.favorite ? "starred" : ""}`}
                      onClick={() => p.onToggleFavorite(s)}
                      aria-label={s.favorite ? `Remove ${s.name} from favorites` : `Add ${s.name} to favorites`}
                      title={s.favorite ? "Remove from favorites" : "Add to favorites"}
                    >
                      {s.favorite ? "★" : "☆"}
                    </button>
                    <button type="button" className="icon" onClick={() => p.onDuplicate(s)} aria-label={`Duplicate ${s.name}`} title="Duplicate">
                      ⧉
                    </button>
                    <button type="button" className="icon" onClick={() => p.onEdit(s)} aria-label={`Edit ${s.name}`} title="Edit">
                      ✎
                    </button>
                    {s.hasSavedSecret && (
                      <button
                        type="button"
                        className="icon"
                        onClick={() => p.onForgetSecret(s)}
                        aria-label={`Forget saved password for ${s.name}`}
                        title="Forget saved password"
                      >
                        ⌫
                      </button>
                    )}
                    <button type="button" className="icon danger" onClick={() => p.onDelete(s)} aria-label={`Delete ${s.name}`} title="Delete">
                      ×
                    </button>
                  </span>
                </li>
              ))}
            </ul>
          </section>
        ))}
      </nav>
      <div className="sidebar-footer">
        <button type="button" className="link" onClick={p.onImport}>
          Import ~/.ssh/config…
        </button>
        <button type="button" className="link" onClick={p.onImportSessions}>
          Import…
        </button>
        <button type="button" className="link" onClick={p.onExport}>
          Export…
        </button>
        <button type="button" className="icon" onClick={p.onSettings} aria-label="Settings" title="Settings">
          ⚙
        </button>
      </div>
    </aside>
  );
}
