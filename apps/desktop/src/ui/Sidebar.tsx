import { type ReactElement, useEffect, useMemo, useRef, useState } from "react";
import type { SessionRecord, ShellChoice } from "../terminal/api";
import { Icon, ProtocolBadge } from "./icons";
import { focusFirstItem, menuKeyDown } from "./menuKeys";
import { MoveDialog } from "./MoveDialog";
import {
  type FolderNode,
  allFolderPaths,
  allSessions,
  authLabel,
  buildTree,
  filterSessions,
  folderDropPath,
  folderPath,
  joinFolder,
  moveTargets,
  normalizeFolder,
  parentFolder,
  targetLabel,
} from "./sessionList";

const COLLAPSED_KEY = "termalith.collapsedFolders";
const FAVORITES = "\u0000favorites";

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
  /** Folders stored on their own (may be empty). */
  folders: string[];
  onConnect: (s: SessionRecord) => void;
  /** SSH sessions: a file browser tab without a shell. */
  onOpenFiles: (s: SessionRecord) => void;
  onEdit: (s: SessionRecord) => void;
  onDelete: (s: SessionRecord) => void;
  onForgetSecret: (s: SessionRecord) => void;
  /** New session, optionally inside a folder. */
  onNew: (folder?: string) => void;
  onQuickConnect: () => void;
  /** Folder changes reject with an error message to keep the editor open. */
  onCreateFolder: (path: string) => Promise<void>;
  onRenameFolder: (from: string, to: string) => Promise<void>;
  onDeleteFolder: (path: string) => void;
  onMoveSession: (s: SessionRecord, folder: string | null) => void;
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

type MenuItem = { label: string; run: () => void; danger?: boolean; disabled?: string } | "sep";
interface MenuState {
  x: number;
  y: number;
  items: MenuItem[];
}
type Editing = { kind: "new"; parent: string } | { kind: "rename"; path: string };
type DragItem = { kind: "session"; session: SessionRecord } | { kind: "folder"; path: string };
interface DragState {
  item: DragItem;
  label: string;
  x: number;
  y: number;
  /** Folder path under the pointer ("" = top level), or null for no valid target. */
  target: string | null;
}

/** Closes a floating menu on outside click or Escape. */
function useDismiss(open: boolean, ref: React.RefObject<HTMLElement | null>, close: () => void) {
  useEffect(() => {
    if (!open) return;
    const down = (e: MouseEvent) => {
      if (!ref.current?.contains(e.target as Node)) close();
    };
    const esc = (e: KeyboardEvent) => e.key === "Escape" && close();
    window.addEventListener("mousedown", down);
    window.addEventListener("keydown", esc);
    window.addEventListener("blur", close);
    return () => {
      window.removeEventListener("mousedown", down);
      window.removeEventListener("keydown", esc);
      window.removeEventListener("blur", close);
    };
  }, [open, ref, close]);
}

/** Inline name field for a new or renamed folder. */
function FolderNameInput({
  initial,
  onSubmit,
  onCancel,
}: {
  initial: string;
  onSubmit: (name: string) => Promise<void>;
  onCancel: () => void;
}) {
  const [value, setValue] = useState(initial);
  const [error, setError] = useState<string | null>(null);
  const busy = useRef(false);
  const submit = async () => {
    if (busy.current) return;
    if (!value.trim()) return onCancel();
    busy.current = true;
    try {
      await onSubmit(value);
    } catch (e) {
      setError(String((e as { message?: string })?.message ?? e));
      busy.current = false;
    }
  };
  return (
    <div className="folder-edit">
      <Icon name="folder" className="folder-icon" />
      <input
        // biome-ignore lint/a11y/noAutofocus: the field appears because the user asked to name a folder
        autoFocus
        value={value}
        aria-label="Folder name"
        aria-invalid={error ? true : undefined}
        title={error ?? "Enter to save, Esc to cancel. Use / for subfolders."}
        onFocus={(e) => e.currentTarget.select()}
        onChange={(e) => {
          setValue(e.target.value);
          setError(null);
        }}
        onKeyDown={(e) => {
          if (e.key === "Enter") void submit();
          if (e.key === "Escape") onCancel();
        }}
        onBlur={() => (error ? onCancel() : void submit())}
      />
      {error && (
        <span className="folder-edit-error" role="alert">
          {error}
        </span>
      )}
    </div>
  );
}

export function Sidebar(p: Props) {
  const [query, setQuery] = useState("");
  const [shellMenu, setShellMenu] = useState(false);
  const [moreMenu, setMoreMenu] = useState(false);
  const [menu, setMenu] = useState<MenuState | null>(null);
  const [editing, setEditing] = useState<Editing | null>(null);
  const [drag, setDrag] = useState<DragState | null>(null);
  const [moving, setMoving] = useState<DragItem | null>(null);
  const shellRef = useRef<HTMLDivElement>(null);
  const moreRef = useRef<HTMLDivElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  /** Set after a drag so the click that ends it does not also connect. */
  const suppressClick = useRef(false);
  useDismiss(shellMenu, shellRef, () => setShellMenu(false));
  useDismiss(moreMenu, moreRef, () => setMoreMenu(false));
  useDismiss(menu !== null, menuRef, () => setMenu(null));
  const folderPaths = useMemo(() => allFolderPaths(p.sessions, p.folders), [p.sessions, p.folders]);

  const searching = query.trim() !== "";
  const filtered = useMemo(() => filterSessions(p.sessions, query), [p.sessions, query]);
  // Empty folders only make sense when not searching.
  const tree = useMemo(() => buildTree(filtered, searching ? [] : p.folders), [filtered, searching, p.folders]);
  const favorites = useMemo(() => filtered.filter((s) => s.favorite), [filtered]);
  const [collapsed, setCollapsed] = useState<Set<string>>(loadCollapsed);
  const setOpen = (path: string, open: boolean) =>
    setCollapsed((c) => {
      if (c.has(path) !== open) return c;
      const next = new Set(c);
      if (open) next.delete(path);
      else next.add(path);
      saveCollapsed(next);
      return next;
    });
  const isOpen = (path: string) => searching || !collapsed.has(path);

  const startNewFolder = (parent: string) => {
    if (parent) setOpen(parent, true);
    setEditing({ kind: "new", parent });
  };

  /** Where focus returns when a context menu closes. */
  const menuOpener = useRef<HTMLElement | null>(null);
  const openMenuAt = (x: number, y: number, items: MenuItem[]) => {
    menuOpener.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    setMenu({
      x: Math.min(x, window.innerWidth - 200),
      y: Math.max(8, Math.min(y, window.innerHeight - 30 * items.length - 16)),
      items,
    });
  };
  const openMenu = (e: React.MouseEvent, items: MenuItem[]) => {
    e.preventDefault();
    e.stopPropagation();
    openMenuAt(e.clientX, e.clientY, items);
  };
  /** Shift+F10 or the context-menu key opens the row's menu below it. */
  const menuKey = (e: React.KeyboardEvent<HTMLElement>, items: () => MenuItem[]) => {
    if (e.key === "ContextMenu" || (e.key === "F10" && e.shiftKey)) {
      e.preventDefault();
      e.stopPropagation();
      const r = e.currentTarget.getBoundingClientRect();
      openMenuAt(r.left + 16, r.bottom, items());
    }
  };
  const closeMenu = () => {
    setMenu(null);
    menuOpener.current?.focus();
  };
  // Focus the first item when a menu opens, so it works from the keyboard.
  useEffect(() => {
    if (menu) focusFirstItem(menuRef.current);
  }, [menu]);

  const sessionMenu = (s: SessionRecord): MenuItem[] => [
    { label: "Connect", run: () => p.onConnect(s) },
    ...((s.protocol ?? "ssh") === "ssh" && !s.filesOnly ? [{ label: "Browse files (SFTP)", run: () => p.onOpenFiles(s) }] : []),
    "sep",
    { label: "Edit…", run: () => p.onEdit(s) },
    { label: "Duplicate", run: () => p.onDuplicate(s) },
    { label: s.favorite ? "Remove from favorites" : "Add to favorites", run: () => p.onToggleFavorite(s) },
    { label: "Move to folder…", run: () => setMoving({ kind: "session", session: s }) },
    ...(s.hasSavedSecret ? [{ label: "Forget saved password", run: () => p.onForgetSecret(s) }] : []),
    "sep",
    { label: "Delete…", run: () => p.onDelete(s), danger: true },
  ];

  const folderMenu = (node: FolderNode): MenuItem[] => {
    const inside = allSessions(node);
    return [
      { label: "New session here…", run: () => p.onNew(node.path) },
      { label: "New subfolder", run: () => startNewFolder(node.path) },
      ...(inside.length > 0
        ? [{ label: `Open all (${inside.length})`, run: () => inside.forEach((x) => p.onConnect(x)) }]
        : []),
      "sep",
      { label: "Rename", run: () => setEditing({ kind: "rename", path: node.path }) },
      { label: "Move to folder…", run: () => setMoving({ kind: "folder", path: node.path }) },
      {
        label: "Delete folder",
        run: () => p.onDeleteFolder(node.path),
        danger: true,
        disabled: inside.length > 0 ? "Move or delete the sessions in it first" : undefined,
      },
    ];
  };

  const moveFolder = (from: string, target: string) => {
    const to = folderDropPath(from, target);
    if (to) void p.onRenameFolder(from, to).catch(() => {});
  };

  // ---- drag and drop (pointer-based: HTML5 DnD conflicts with the window's file drop)
  const pending = useRef<{ item: DragItem; label: string; x: number; y: number } | null>(null);
  const dropTarget = (x: number, y: number, item: DragItem): string | null => {
    const el = document.elementFromPoint(x, y)?.closest<HTMLElement>("[data-drop]");
    if (!el) return null;
    const target = el.dataset.drop ?? "";
    if (item.kind === "folder") return folderDropPath(item.path, target) ? target : null;
    return normalizeFolder(item.session.folder).toLowerCase() === target.toLowerCase() ? null : target;
  };
  /** Mirrors `drag` so pointer handlers never see a stale value. */
  const dragRef = useRef<DragState | null>(null);
  const updateDrag = (d: DragState | null) => {
    dragRef.current = d;
    setDrag(d);
  };
  useEffect(() => {
    const move = (e: PointerEvent) => {
      const start = pending.current;
      if (!start) return;
      if (!dragRef.current && Math.hypot(e.clientX - start.x, e.clientY - start.y) < 6) return;
      updateDrag({ ...start, x: e.clientX, y: e.clientY, target: dropTarget(e.clientX, e.clientY, start.item) });
    };
    const up = (e: PointerEvent) => {
      pending.current = null;
      const current = dragRef.current;
      if (!current) return;
      suppressClick.current = true;
      setTimeout(() => {
        suppressClick.current = false;
      }, 0);
      const target = dropTarget(e.clientX, e.clientY, current.item);
      updateDrag(null);
      if (target === null) return;
      if (current.item.kind === "session") p.onMoveSession(current.item.session, target || null);
      else moveFolder(current.item.path, target);
    };
    const cancel = (e: KeyboardEvent) => {
      if (e.key === "Escape" && pending.current) {
        pending.current = null;
        updateDrag(null);
      }
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    window.addEventListener("keydown", cancel);
    return () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      window.removeEventListener("keydown", cancel);
    };
  });
  const dragStart = (e: React.PointerEvent, item: DragItem, label: string) => {
    if (e.button !== 0 || searching) return;
    pending.current = { item, label, x: e.clientX, y: e.clientY };
  };
  const guardClick = (run: () => void) => () => {
    if (!suppressClick.current) run();
  };

  const sessionRow = (s: SessionRecord, inFavorites = false) => {
    const protocol = s.filesOnly ? "sftp" : s.mosh ? "mosh" : (s.protocol ?? "ssh");
    const jump = s.jumpVia ? p.sessions.find((j) => j.id === s.jumpVia)?.name ?? "?" : null;
    const extras = [authLabel(s) && authLabel(s) !== protocol.toUpperCase() ? authLabel(s) : "", jump ? `via ${jump}` : "", s.forwardAgent ? "agent fwd" : "", s.x11Forward ? "X11" : ""].filter(Boolean);
    return (
      <li
        key={`${inFavorites ? "fav-" : ""}${s.id}`}
        className={`session-item ${drag?.item.kind === "session" && drag.item.session.id === s.id ? "dragging" : ""}`}
        onContextMenu={(e) => openMenu(e, sessionMenu(s))}
      >
        <button
          type="button"
          className="session"
          data-nav="session"
          onClick={guardClick(() => p.onConnect(s))}
          onKeyDown={(e) => menuKey(e, () => sessionMenu(s))}
          onPointerDown={inFavorites ? undefined : (e) => dragStart(e, { kind: "session", session: s }, s.name)}
          title={`Connect to ${targetLabel(s)}`}
          aria-label={`${s.name}, ${protocol.toUpperCase()} ${targetLabel(s)}${extras.length ? `, ${extras.join(", ")}` : ""}`}
        >
          <ProtocolBadge protocol={protocol} />
          <span className="session-text">
            <span className="session-name">
              {s.name}
              {s.favorite && !inFavorites && <Icon name="star" size={11} className="session-star" />}
            </span>
            <span className="session-target">
              {targetLabel(s)}
              {extras.length > 0 && <span className="session-extra">{extras.join(", ")}</span>}
            </span>
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
            <Icon name="star" size={14} />
          </button>
          {protocol === "ssh" && (
            <button type="button" className="icon" onClick={() => p.onOpenFiles(s)} aria-label={`Browse files on ${s.name}`} title="Browse files (SFTP)">
              <Icon name="files" size={14} />
            </button>
          )}
          <button type="button" className="icon" onClick={() => p.onEdit(s)} aria-label={`Edit ${s.name}`} title="Edit">
            <Icon name="edit" size={14} />
          </button>
          <button
            type="button"
            className="icon"
            aria-label={`More actions for ${s.name}`}
            title="More"
            onClick={(e) => openMenu(e, sessionMenu(s))}
          >
            <Icon name="more" size={14} />
          </button>
        </span>
      </li>
    );
  };

  /** Arrow keys move between rows; Right/Left open and close folders. */
  const treeKey = (e: React.KeyboardEvent<HTMLElement>) => {
    const row = e.target as HTMLElement;
    if (!row.dataset.nav) return;
    const rows = [...e.currentTarget.querySelectorAll<HTMLElement>("[data-nav]")];
    const i = rows.indexOf(row);
    const go = (el: HTMLElement | null | undefined) => {
      e.preventDefault();
      el?.focus();
    };
    const path = row.dataset.path;
    const isFolder = row.dataset.nav === "folder" && path !== undefined;
    const open = row.getAttribute("aria-expanded") === "true";
    // The folder this row is in: for a folder row, skip its own <li>.
    const parentToggle = () =>
      (isFolder ? row.closest("li.folder")?.parentElement : row)
        ?.closest("li.folder")
        ?.querySelector<HTMLElement>(":scope > .folder-head [data-nav]");
    switch (e.key) {
      case "ArrowDown":
        return go(rows[i + 1]);
      case "ArrowUp":
        return go(rows[i - 1]);
      case "Home":
        return go(rows[0]);
      case "End":
        return go(rows.at(-1));
      case "ArrowRight":
        if (!isFolder) return;
        if (!open) {
          e.preventDefault();
          setOpen(path, true);
        } else go(rows[i + 1]);
        return;
      case "ArrowLeft":
        if (isFolder && open) {
          e.preventDefault();
          setOpen(path, false);
        } else go(parentToggle());
        return;
    }
  };

  const newFolderRow = (parent: string) =>
    editing?.kind === "new" && editing.parent === parent ? (
      <li key="\u0000new" className="folder">
        <FolderNameInput
          initial="New folder"
          onCancel={() => setEditing(null)}
          onSubmit={async (name) => {
            await p.onCreateFolder(joinFolder(parent, name));
            setEditing(null);
          }}
        />
      </li>
    ) : null;

  /** A folder header that opens and closes, then its subfolders and sessions. */
  const folder = (node: FolderNode, favorites = false): ReactElement => {
    const open = isOpen(node.path);
    const renaming = editing?.kind === "rename" && editing.path === node.path;
    const isTarget = drag?.target === node.path;
    return (
      <li key={node.path} className={`folder ${isTarget ? "drop-target" : ""}`}>
        {renaming ? (
          <FolderNameInput
            initial={node.name}
            onCancel={() => setEditing(null)}
            onSubmit={async (name) => {
              const to = joinFolder(parentFolder(node.path), name);
              if (to !== node.path) await p.onRenameFolder(node.path, to);
              setEditing(null);
            }}
          />
        ) : (
          <div
            className="folder-head"
            data-drop={favorites ? undefined : node.path}
            onContextMenu={favorites ? undefined : (e) => openMenu(e, folderMenu(node))}
          >
            <button
              type="button"
              className="folder-toggle"
              aria-expanded={open}
              aria-label={`${node.name} folder, ${node.total} session${node.total === 1 ? "" : "s"}`}
              data-nav="folder"
              data-path={node.path}
              onClick={guardClick(() => setOpen(node.path, !open))}
              onKeyDown={favorites ? undefined : (e) => menuKey(e, () => folderMenu(node))}
              onPointerDown={favorites ? undefined : (e) => dragStart(e, { kind: "folder", path: node.path }, node.name)}
              title={open ? "Collapse" : "Expand"}
            >
              <Icon name="chevron" size={12} className={`caret ${open ? "open" : ""}`} />
              {favorites ? (
                <Icon name="star" className="folder-icon fav" />
              ) : (
                <Icon name={open ? "folderOpen" : "folder"} className="folder-icon" />
              )}
              <span className="folder-name">{node.name}</span>
              {node.total > 0 && <span className="folder-count">{node.total}</span>}
            </button>
            {!favorites && (
              <button
                type="button"
                className="icon folder-more"
                aria-label={`Actions for folder ${node.name}`}
                title="Folder actions"
                onClick={(e) => openMenu(e, folderMenu(node))}
              >
                <Icon name="more" size={14} />
              </button>
            )}
          </div>
        )}
        {open && (
          <ul className="folder-children">
            {newFolderRow(node.path)}
            {node.folders.map((f) => folder(f))}
            {node.sessions.map((s) => sessionRow(s, favorites))}
            {!favorites && node.folders.length === 0 && node.sessions.length === 0 && editing?.kind !== "new" && (
              <li className="folder-empty">
                Empty. Drag sessions here or{" "}
                <button type="button" className="link" onClick={() => p.onNew(node.path)}>
                  add one
                </button>
                .
              </li>
            )}
          </ul>
        )}
      </li>
    );
  };

  return (
    <aside className="sidebar" aria-label="Sessions">
      <div className="sidebar-actions">
        <button type="button" className="primary" onClick={p.onQuickConnect}>
          <Icon name="plug" size={14} />
          Quick connect
        </button>
        <div className="split-button" ref={shellRef}>
          <button type="button" onClick={() => p.onLocalTerminal()} title="Open a local terminal">
            <Icon name="terminal" size={14} />
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
              <Icon name="chevron" size={10} className="caret open" />
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
        <span className="sidebar-head-actions">
          <button type="button" className="icon" onClick={() => startNewFolder("")} aria-label="New folder" title="New folder">
            <Icon name="folderPlus" />
          </button>
          <button type="button" className="icon" onClick={() => p.onNew()} aria-label="New session" title="New session">
            <Icon name="plus" />
          </button>
        </span>
      </div>
      <label className="search">
        <Icon name="search" size={14} />
        <input
          type="search"
          placeholder="Search, or #tag"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          aria-label="Search sessions"
        />
      </label>
      <nav
        className={`session-list ${drag?.target === "" ? "drop-target" : ""} ${drag ? "is-dragging" : ""}`}
        aria-label="Saved sessions (arrow keys move, Shift+F10 opens actions)"
        onKeyDown={treeKey}
        data-drop=""
        onContextMenu={(e) =>
          openMenu(e, [
            { label: "New session…", run: () => p.onNew() },
            { label: "New folder", run: () => startNewFolder("") },
          ])
        }
      >
        {p.sessions.length === 0 && p.folders.length === 0 && !editing && (
          <div className="empty">
            <p>No saved sessions yet.</p>
            <button type="button" className="primary" onClick={() => p.onNew()}>
              New session
            </button>
            <button type="button" className="link" onClick={p.onImport}>
              Import from ~/.ssh/config
            </button>
          </div>
        )}
        {p.sessions.length > 0 && searching && filtered.length === 0 && <p className="empty">No matches.</p>}
        <ul className="tree">
          {newFolderRow("")}
          {favorites.length > 0 &&
            folder({ name: "Favorites", path: FAVORITES, folders: [], sessions: favorites, total: favorites.length }, true)}
          {tree.folders.map((f) => folder(f))}
          {tree.sessions.map((s) => sessionRow(s))}
        </ul>
      </nav>
      <div className="sidebar-footer">
        <button type="button" className="ghost" onClick={p.onTools} title="Ping, traceroute, port scan, DNS…">
          <Icon name="tools" size={14} />
          Tools
        </button>
        <button type="button" className="ghost" onClick={p.onKeys} title="SSH keys">
          <Icon name="key" size={14} />
          Keys
        </button>
        <div className="split-button" ref={moreRef}>
          <button
            type="button"
            className="ghost"
            aria-haspopup="menu"
            aria-expanded={moreMenu}
            title="Import and export sessions"
            onClick={() => setMoreMenu((v) => !v)}
          >
            <Icon name="swap" size={14} />
            Import
          </button>
          {moreMenu && (
            <div className="menu menu-up" role="menu">
              {(
                [
                  ["Import ~/.ssh/config…", p.onImport],
                  ["Import sessions (JSON, MobaXterm)…", p.onImportSessions],
                  ["Export sessions…", p.onExport],
                ] as const
              ).map(([label, run]) => (
                <button
                  key={label}
                  type="button"
                  role="menuitem"
                  onClick={() => {
                    setMoreMenu(false);
                    run();
                  }}
                >
                  {label}
                </button>
              ))}
            </div>
          )}
        </div>
        <span className="sidebar-footer-end">
          <button type="button" className="icon" onClick={p.onSettings} aria-label="Settings" title="Settings">
            <Icon name="gear" />
          </button>
          <button type="button" className="icon" onClick={p.onAbout} aria-label="About Termalith" title="About Termalith">
            <Icon name="info" />
          </button>
        </span>
      </div>
      {menu && (
        <div
          ref={menuRef}
          className="menu context-menu"
          role="menu"
          style={{ left: menu.x, top: menu.y }}
          onKeyDown={(e) => menuKeyDown(e, closeMenu)}
        >
          {menu.items.map((item, i) =>
            item === "sep" ? (
              // biome-ignore lint/suspicious/noArrayIndexKey: separators have no identity
              <hr key={`sep-${i}`} />
            ) : (
              <button
                key={item.label}
                type="button"
                role="menuitem"
                className={item.danger ? "danger" : undefined}
                disabled={item.disabled !== undefined}
                title={item.disabled}
                onClick={() => {
                  closeMenu();
                  item.run();
                }}
              >
                {item.label}
              </button>
            ),
          )}
        </div>
      )}
      {moving && (
        <MoveDialog
          what={moving.kind === "session" ? `"${moving.session.name}"` : `folder "${folderPath(moving.path).at(-1)}"`}
          targets={moveTargets(
            folderPaths,
            moving.kind === "session" ? { kind: "session", folder: moving.session.folder } : moving,
          )}
          onCancel={() => setMoving(null)}
          onMove={(target) => {
            if (moving.kind === "session") p.onMoveSession(moving.session, target || null);
            else moveFolder(moving.path, target);
            setMoving(null);
          }}
        />
      )}
      {drag && (
        <div className="drag-ghost" style={{ left: drag.x + 12, top: drag.y + 8 }} aria-hidden="true">
          {drag.item.kind === "folder" ? <Icon name="folder" size={13} /> : <ProtocolBadge protocol={drag.item.session.filesOnly ? "sftp" : (drag.item.session.protocol ?? "ssh")} />}
          {drag.label}
          <span className="drag-hint">
            {drag.target === null ? "" : drag.target ? `→ ${folderPath(drag.target).at(-1)}` : "→ top level"}
          </span>
        </div>
      )}
    </aside>
  );
}
