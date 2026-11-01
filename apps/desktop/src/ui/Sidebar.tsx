import { useEffect, useMemo, useRef, useState } from "react";
import type { SessionRecord, ShellChoice } from "../terminal/api";
import { authLabel, filterSessions, groupSessions, targetLabel } from "./sessionList";

interface Props {
  sessions: SessionRecord[];
  onConnect: (s: SessionRecord) => void;
  onEdit: (s: SessionRecord) => void;
  onDelete: (s: SessionRecord) => void;
  onForgetSecret: (s: SessionRecord) => void;
  onNew: () => void;
  onQuickConnect: () => void;
  /** Installed shells; `undefined` id = the preferred one. */
  shells: ShellChoice[];
  onLocalTerminal: (shellId?: string) => void;
  onImport: () => void;
  onToggleFavorite: (s: SessionRecord) => void;
  onDuplicate: (s: SessionRecord) => void;
  onExport: () => void;
  onImportSessions: () => void;
  onSettings: () => void;
  onSerial: () => void;
  onTools: () => void;
}

export function Sidebar(p: Props) {
  const [query, setQuery] = useState("");
  const [shellMenu, setShellMenu] = useState(false);
  const menuRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!shellMenu) return;
    const close = (e: MouseEvent) => {
      if (!menuRef.current?.contains(e.target as Node)) setShellMenu(false);
    };
    const esc = (e: KeyboardEvent) => e.key === "Escape" && setShellMenu(false);
    window.addEventListener("mousedown", close);
    window.addEventListener("keydown", esc);
    return () => {
      window.removeEventListener("mousedown", close);
      window.removeEventListener("keydown", esc);
    };
  }, [shellMenu]);
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
        <div className="split-button" ref={menuRef}>
          <button type="button" onClick={() => p.onLocalTerminal()} title="Open a local terminal">
            Local
          </button>
          {p.shells.length > 1 && (
            <button
              type="button"
              className="split-toggle"
              aria-haspopup="menu"
              aria-expanded={shellMenu}
              aria-label="Choose a shell"
              title="Choose a shell"
              onClick={() => setShellMenu((v) => !v)}
            >
              ▾
            </button>
          )}
          {shellMenu && (
            <div className="menu" role="menu">
              {p.shells.map((s) => (
                <button
                  key={s.id}
                  type="button"
                  role="menuitem"
                  title={s.id}
                  onClick={() => {
                    setShellMenu(false);
                    p.onLocalTerminal(s.id);
                  }}
                >
                  {s.name}
                </button>
              ))}
            </div>
          )}
        </div>
        <button type="button" onClick={p.onSerial} title="Open a serial console">
          Serial
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
        placeholder="Search (#tag)"
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
                      {s.forwardAgent && " · agent fwd"}
                    </span>
                  </button>
                  {(s.tags?.length ?? 0) > 0 && (
                    <span className="session-tags">
                      {s.tags?.map((t) => (
                        <button
                          key={t}
                          type="button"
                          className="tag"
                          title={`Show sessions tagged ${t}`}
                          onClick={() => setQuery(`#${t}`)}
                        >
                          #{t}
                        </button>
                      ))}
                    </span>
                  )}
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
        <button type="button" className="link" onClick={p.onTools}>
          Network tools
        </button>
        <button type="button" className="icon" onClick={p.onSettings} aria-label="Settings" title="Settings">
          ⚙
        </button>
      </div>
    </aside>
  );
}
