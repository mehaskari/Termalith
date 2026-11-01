import { type ReactElement, useEffect, useMemo, useRef, useState } from "react";
import type { SessionRecord, ShellChoice } from "../terminal/api";
import { type FolderNode, allSessions, authLabel, buildTree, filterSessions, targetLabel } from "./sessionList";

const COLLAPSED_KEY = "termalith.collapsedFolders";

/** Closed folders are remembered per device (a convenience, not data). */
function loadCollapsed(): Set<string> {
  try {
    const v = JSON.parse(localStorage.getItem(COLLAPSED_KEY) ?? "[]");
    return new Set(Array.isArray(v) ? v.filter((x) => typeof x === "string") : []);
  } catch {
    return new Set();
  }
}

function saveCollapsed(paths: Set<string>) {
  try {
    localStorage.setItem(COLLAPSED_KEY, JSON.stringify([...paths]));
  } catch {
    // Storage unavailable: folders just reopen next time.
  }
}

interface Props {
  sessions: SessionRecord[];
  onConnect: (s: SessionRecord) => void;
  /** SSH sessions: a file browser tab without a shell. */
  onOpenFiles: (s: SessionRecord) => void;
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
  onKeys: () => void;
  onAbout: () => void;
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
  const tree = useMemo(() => buildTree(filtered), [filtered]);
  const favorites = useMemo(() => filtered.filter((s) => s.favorite), [filtered]);
  const [collapsed, setCollapsed] = useState<Set<string>>(loadCollapsed);
  const searching = query.trim() !== "";
  const toggle = (path: string) =>
    setCollapsed((c) => {
      const next = new Set(c);
      if (!next.delete(path)) next.add(path);
      saveCollapsed(next);
      return next;
    });

  const sessionRow = (s: SessionRecord) => (
    <li key={s.id}>
      <button type="button" className="session" onClick={() => p.onConnect(s)} title={`Connect to ${targetLabel(s)}`}>
        <span className="session-name">{s.name}</span>
        <span className="session-target">
          {targetLabel(s)}
          {authLabel(s) && ` · ${authLabel(s)}`}
          {s.jumpVia && ` · via ${p.sessions.find((j) => j.id === s.jumpVia)?.name ?? "?"}`}
          {s.forwardAgent && " · agent fwd"}
          {s.x11Forward && " · X11"}
        </span>
      </button>
      {(s.tags?.length ?? 0) > 0 && (
        <span className="session-tags">
          {s.tags?.map((t) => (
            <button key={t} type="button" className="tag" title={`Show sessions tagged ${t}`} onClick={() => setQuery(`#${t}`)}>
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
        {(s.protocol ?? "ssh") === "ssh" && (
          <button type="button" className="icon" onClick={() => p.onOpenFiles(s)} aria-label={`Browse files on ${s.name}`} title="Browse files (SFTP)">
            ⇅
          </button>
        )}
        <button type="button" className="icon" onClick={() => p.onDuplicate(s)} aria-label={`Duplicate ${s.name}`} title="Duplicate">
          ⧉
        </button>
        <button type="button" className="icon" onClick={() => p.onEdit(s)} aria-label={`Edit ${s.name}`} title="Edit">
          ✎
        </button>
        {s.hasSavedSecret && (
          <button type="button" className="icon" onClick={() => p.onForgetSecret(s)} aria-label={`Forget saved password for ${s.name}`} title="Forget saved password">
            ⌫
          </button>
        )}
        <button type="button" className="icon danger" onClick={() => p.onDelete(s)} aria-label={`Delete ${s.name}`} title="Delete">
          ×
        </button>
      </span>
    </li>
  );

  /** A folder header that opens and closes, then its subfolders and sessions. */
  const folder = (node: FolderNode, label = node.name, sessions = allSessions(node)): ReactElement => {
    const open = searching || !collapsed.has(node.path);
    return (
      <li key={node.path} className="folder">
        <div className="folder-head">
          <button
            type="button"
            className="folder-toggle"
            aria-expanded={open}
            onClick={() => toggle(node.path)}
            title={open ? "Collapse" : "Expand"}
          >
            <span className="caret" aria-hidden="true">{open ? "▾" : "▸"}</span>
            <span className="folder-icon" aria-hidden="true">{open ? "📂" : "📁"}</span>
            <span className="folder-name">{label}</span>
            <span className="folder-count">{node.total}</span>
          </button>
          {sessions.length > 1 && (
            <button
              type="button"
              className="link folder-open-all"
              title={`Open all ${sessions.length} sessions`}
              aria-label={`Open all sessions in ${label}`}
              onClick={() => sessions.forEach((x) => p.onConnect(x))}
            >
              Open all
            </button>
          )}
        </div>
        {open && (
          <ul className="folder-children">
            {node.folders.map((f) => folder(f))}
            {node.sessions.map(sessionRow)}
          </ul>
        )}
      </li>
    );
  };

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
        {p.sessions.length > 0 && filtered.length === 0 && <p className="empty">No matches.</p>}
        <ul className="tree">
          {favorites.length > 0 &&
            folder(
              { name: "Favorites", path: "\u0000favorites", folders: [], sessions: favorites, total: favorites.length },
              "★ Favorites",
            )}
          {tree.folders.map((f) => folder(f))}
          {tree.sessions.map(sessionRow)}
        </ul>
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
        <button type="button" className="link" onClick={p.onKeys}>
          SSH keys
        </button>
        <button type="button" className="icon" onClick={p.onAbout} aria-label="About Termalith" title="About Termalith">
          ⓘ
        </button>
        <button type="button" className="icon" onClick={p.onSettings} aria-label="Settings" title="Settings">
          ⚙
        </button>
      </div>
    </aside>
  );
}
