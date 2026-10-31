import { useMemo, useState } from "react";
import type { SessionRecord } from "../terminal/api";
import { filterSessions, groupSessions, targetLabel } from "./sessionList";

interface Props {
  sessions: SessionRecord[];
  onConnect: (s: SessionRecord) => void;
  onEdit: (s: SessionRecord) => void;
  onDelete: (s: SessionRecord) => void;
  onForgetSecret: (s: SessionRecord) => void;
  onNew: () => void;
  onQuickConnect: () => void;
  onLocalTerminal: () => void;
}

export function Sidebar(p: Props) {
  const [query, setQuery] = useState("");
  const groups = useMemo(() => groupSessions(filterSessions(p.sessions, query)), [p.sessions, query]);

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
          <section key={g.folder ?? "\u0000"}>
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
                      {s.auth.kind === "keyFile" ? " · key" : s.hasSavedSecret ? " · saved" : ""}
                    </span>
                  </button>
                  <span className="row-actions">
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
    </aside>
  );
}
