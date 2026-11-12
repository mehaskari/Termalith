import type { SessionInput, SessionProtocol, SessionRecord } from "../terminal/api";

export interface SessionGroup {
  folder: string | null;
  sessions: SessionRecord[];
}

/**
 * Case-insensitive search. Words starting with `#` must match a tag (prefix);
 * other words must appear in the name, host, user, folder or a tag.
 */
export function filterSessions(sessions: SessionRecord[], query: string): SessionRecord[] {
  const words = query.trim().toLowerCase().split(/\s+/).filter(Boolean);
  if (!words.length) return sessions;
  return sessions.filter((s) => {
    const tags = (s.tags ?? []).map((t) => t.toLowerCase());
    const fields = [s.name, s.host, s.username, s.folder ?? "", `${s.username}@${s.host}`, ...tags].map((f) =>
      f.toLowerCase(),
    );
    return words.every((w) =>
      w.startsWith("#") && w.length > 1
        ? tags.some((t) => t.startsWith(w.slice(1)))
        : fields.some((f) => f.includes(w)),
    );
  });
}

/** Splits "prod, web #db" style input into tags (backend normalizes again). */
export function parseTags(input: string): string[] {
  const out: string[] = [];
  for (const raw of input.split(",")) {
    const t = raw.trim().replace(/^#+/, "").trim();
    if (t && !out.some((o) => o.toLowerCase() === t.toLowerCase())) out.push(t);
  }
  return out;
}

/** Groups by folder: named folders alphabetically, then ungrouped. */
export function groupSessions(sessions: SessionRecord[]): SessionGroup[] {
  const byFolder = new Map<string | null, SessionRecord[]>();
  for (const s of sessions) {
    const key = s.folder?.trim() || null;
    const list = byFolder.get(key) ?? [];
    list.push(s);
    byFolder.set(key, list);
  }
  const named = [...byFolder.keys()]
    .filter((k): k is string => k !== null)
    .sort((a, b) => a.localeCompare(b, undefined, { sensitivity: "base" }));
  const byName = (a: SessionRecord, b: SessionRecord) =>
    a.name.localeCompare(b.name, undefined, { sensitivity: "base" });
  const groups: SessionGroup[] = named.map((folder) => ({
    folder,
    sessions: (byFolder.get(folder) ?? []).sort(byName),
  }));
  const loose = byFolder.get(null);
  if (loose) groups.push({ folder: null, sessions: loose.sort(byName) });
  return groups;
}

export const DEFAULT_PORT: Record<SessionProtocol, number> = { ssh: 22, rdp: 3389, vnc: 5900, telnet: 23, ftp: 21, ftps: 21 };

export function isFtp(s: Pick<SessionInput, "protocol">): boolean {
  return s.protocol === "ftp" || s.protocol === "ftps";
}

/** Opens a terminal (SSH shell, Telnet or Mosh), so it can take broadcast input. */
export function opensTerminal(s: Pick<SessionInput, "protocol" | "filesOnly">): boolean {
  const protocol = s.protocol ?? "ssh";
  return (protocol === "ssh" && !s.filesOnly) || protocol === "telnet";
}

export function isDesktop(s: Pick<SessionInput, "protocol">): boolean {
  return s.protocol === "rdp" || s.protocol === "vnc";
}

export interface FolderNode {
  /** Last path segment, e.g. "DB". */
  name: string;
  /** Full path, e.g. "Production/DB" (the key for open/closed state). */
  path: string;
  folders: FolderNode[];
  sessions: SessionRecord[];
  /** Sessions in this folder and every subfolder. */
  total: number;
}

/** Splits "Production / DB" or "Production/DB" into ["Production", "DB"]. */
export function folderPath(folder: string | null | undefined): string[] {
  return (folder ?? "")
    .split("/")
    .map((p) => p.trim())
    .filter(Boolean);
}

/** "A / B" → "A/B" (the backend normalizes the same way). */
export const normalizeFolder = (folder: string | null | undefined): string => folderPath(folder).join("/");

/** Parent of "A/B/C" is "A/B"; of "A" it is "" (top level). */
export function parentFolder(path: string): string {
  const parts = folderPath(path);
  return parts.slice(0, -1).join("/");
}

export const joinFolder = (parent: string, name: string): string => normalizeFolder(`${parent}/${name}`);

/** True if `path` is `folder` or inside it (case-insensitive, like the tree). */
export function isInside(path: string, folder: string): boolean {
  const p = normalizeFolder(path).toLowerCase();
  const f = normalizeFolder(folder).toLowerCase();
  return p === f || p.startsWith(`${f}/`);
}

/** Where a folder ends up when dropped on `target` ("" = top level), or null if the move is not possible. */
export function folderDropPath(from: string, target: string): string | null {
  if (target && isInside(target, from)) return null;
  if (parentFolder(from).toLowerCase() === normalizeFolder(target).toLowerCase()) return null;
  return joinFolder(target, folderPath(from).at(-1) ?? "");
}

/**
 * Where a session (its current folder) or a folder can be moved, for the
 * keyboard alternative to drag and drop. "" is the top level.
 */
export function moveTargets(
  allPaths: string[],
  item: { kind: "session"; folder: string | null | undefined } | { kind: "folder"; path: string },
): string[] {
  const targets = ["", ...allPaths];
  if (item.kind === "folder") return targets.filter((t) => folderDropPath(item.path, t) !== null);
  const current = normalizeFolder(item.folder ?? "").toLowerCase();
  return targets.filter((t) => t.toLowerCase() !== current);
}

/** Every folder path, including parents and empty stored folders, sorted. */
export function allFolderPaths(sessions: SessionRecord[], stored: string[] = []): string[] {
  const seen = new Map<string, string>();
  for (const f of [...stored, ...sessions.map((s) => s.folder ?? "")]) {
    // Children keep the spelling their parent was first seen with, like the tree.
    let parent = "";
    for (const part of folderPath(f)) {
      const p = parent ? `${parent}/${part}` : part;
      if (!seen.has(p.toLowerCase())) seen.set(p.toLowerCase(), p);
      parent = seen.get(p.toLowerCase()) as string;
    }
  }
  return [...seen.values()].sort((a, b) => a.localeCompare(b, undefined, { sensitivity: "base" }));
}

/**
 * Nested folders (MobaXterm-style tree). Folders sort alphabetically
 * before loose sessions at each level; sessions sort by name. `stored`
 * adds folders that have no sessions (yet).
 */
export function buildTree(sessions: SessionRecord[], stored: string[] = []): FolderNode {
  const root: FolderNode = { name: "", path: "", folders: [], sessions: [], total: 0 };
  const byName = (a: { name: string }, b: { name: string }) =>
    a.name.localeCompare(b.name, undefined, { sensitivity: "base" });
  const walk = (folder: string | null | undefined, count: boolean): FolderNode => {
    let node = root;
    if (count) node.total++;
    for (const part of folderPath(folder)) {
      let child = node.folders.find((f) => f.name.toLowerCase() === part.toLowerCase());
      if (!child) {
        child = { name: part, path: node.path ? `${node.path}/${part}` : part, folders: [], sessions: [], total: 0 };
        node.folders.push(child);
      }
      node = child;
      if (count) node.total++;
    }
    return node;
  };
  for (const f of stored) walk(f, false);
  for (const s of sessions) walk(s.folder, true).sessions.push(s);
  const sort = (n: FolderNode) => {
    n.folders.sort(byName);
    n.sessions.sort(byName);
    n.folders.forEach(sort);
  };
  sort(root);
  return root;
}

/** Every session in a folder and its subfolders. */
export function allSessions(node: FolderNode): SessionRecord[] {
  return [...node.sessions, ...node.folders.flatMap(allSessions)];
}

export function authLabel(s: Pick<SessionRecord, "auth" | "hasSavedSecret" | "protocol">): string {
  if (s.protocol === "rdp") return s.hasSavedSecret ? "saved" : "RDP";
  if (s.protocol === "vnc") return "VNC";
  if (s.protocol === "telnet") return "Telnet";
  if (isFtp(s)) return s.hasSavedSecret ? "saved" : (s.protocol ?? "").toUpperCase();
  if (s.auth.kind === "keyFile") return "key";
  if (s.auth.kind === "agent") return "agent";
  return s.hasSavedSecret ? "saved" : "";
}

export function targetLabel(s: Pick<SessionInput, "username" | "host" | "port" | "protocol">): string {
  const protocol = s.protocol ?? "ssh";
  const host = s.port === DEFAULT_PORT[protocol] ? s.host : `${s.host}:${s.port}`;
  return s.username ? `${s.username}@${host}` : host;
}

export interface SessionFormState {
  protocol: SessionProtocol;
  name: string;
  host: string;
  port: string;
  username: string;
  authKind: "password" | "keyFile" | "agent";
  keyPath: string;
  folder: string;
  /** Session id of the jump host, or "" for a direct connection. */
  jumpVia: string;
  /** RDP only. */
  domain: string;
  /** Comma-separated. */
  tags: string;
  forwardAgent: boolean;
  x11Forward: boolean;
  startupCommand: string;
  filesOnly: boolean;
  mosh: boolean;
  /** Appearance: "" follows the settings. */
  lookScheme: string;
  lookFontSize: string;
  lookFont: string;
}

/** Switches protocol, moving the port along if it was the old default. */
export function withProtocol(f: SessionFormState, protocol: SessionProtocol): SessionFormState {
  const port = f.port === String(DEFAULT_PORT[f.protocol]) || !f.port ? String(DEFAULT_PORT[protocol]) : f.port;
  return { ...f, protocol, port, authKind: protocol === "ssh" ? f.authKind : "password" };
}

export function emptyForm(): SessionFormState {
  return {
    protocol: "ssh",
    name: "",
    host: "",
    port: "22",
    username: "",
    authKind: "password",
    keyPath: "",
    folder: "",
    jumpVia: "",
    domain: "",
    tags: "",
    forwardAgent: false,
    x11Forward: false,
    startupCommand: "",
    filesOnly: false,
    mosh: false,
    lookScheme: "",
    lookFontSize: "",
    lookFont: "",
  };
}

export function formFromSession(s: SessionInput): SessionFormState {
  return {
    protocol: s.protocol ?? "ssh",
    name: s.name,
    host: s.host,
    port: String(s.port),
    username: s.username,
    authKind: s.auth.kind,
    keyPath: s.auth.kind === "keyFile" ? s.auth.path : "",
    folder: s.folder ?? "",
    jumpVia: s.jumpVia ?? "",
    domain: s.domain ?? "",
    tags: (s.tags ?? []).join(", "),
    forwardAgent: s.forwardAgent ?? false,
    x11Forward: s.x11Forward ?? false,
    startupCommand: s.startupCommand ?? "",
    filesOnly: s.filesOnly ?? false,
    mosh: s.mosh ?? false,
    lookScheme: s.appearance?.terminalScheme ?? "",
    lookFontSize: s.appearance?.fontSize ? String(s.appearance.fontSize) : "",
    lookFont: s.appearance?.fontFamily ?? "",
  };
}

/** Fills the form from a typed command such as `ssh -p 2222 me@host`. */
export function applyTypedConnection(
  f: SessionFormState,
  t: { protocol: SessionProtocol; mosh?: boolean; filesOnly?: boolean; user?: string; host: string; port?: number; keyPath?: string },
): SessionFormState {
  const next = withProtocol(f, t.protocol);
  return {
    ...next,
    host: t.host,
    port: t.port ? String(t.port) : next.port,
    username: t.user ?? next.username,
    mosh: t.mosh ?? false,
    filesOnly: t.filesOnly ?? false,
    ...(t.keyPath ? { authKind: "keyFile" as const, keyPath: t.keyPath } : {}),
  };
}

/**
 * Splits `user@host:port` typed into the host field. IPv6 literals need
 * brackets for a port (`[fe80::1]:2222`); a bare IPv6 address is a host.
 */
export function splitTarget(raw: string): { user?: string; host: string; port?: number } {
  let rest = raw.trim();
  let user: string | undefined;
  const at = rest.lastIndexOf("@");
  if (at > 0) {
    user = rest.slice(0, at);
    rest = rest.slice(at + 1);
  }
  const bracketed = /^\[([^\]]+)\](?::(\d+))?$/.exec(rest);
  if (bracketed) return { user, host: bracketed[1] as string, port: bracketed[2] ? Number(bracketed[2]) : undefined };
  const m = /^([^:]+):(\d+)$/.exec(rest);
  if (m) return { user, host: m[1] as string, port: Number(m[2]) };
  return { user, host: rest };
}

/**
 * Converts the form to a SessionInput, or returns a field error. The backend
 * re-validates; this only gives faster feedback.
 */
export function parseForm(
  f: SessionFormState,
  { requireName }: { requireName: boolean },
): { ok: true; value: SessionInput } | { ok: false; field: string; message: string } {
  // "user@host:port" in the host field fills the other two.
  const typed = splitTarget(f.host);
  const host = typed.host;
  const username = f.username.trim() || typed.user?.trim() || "";
  const port =
    typed.port !== undefined && (f.port === "" || f.port === String(DEFAULT_PORT[f.protocol])) ? typed.port : Number(f.port);
  if (!host) return { ok: false, field: "host", message: "Host is required" };
  if (/\s/.test(host) || host.startsWith("-"))
    return { ok: false, field: "host", message: "Not a valid host name or address" };
  // Everything but SSH: optional user name, no keys, no SSH-only options.
  const desktop = f.protocol !== "ssh";
  const shell = !desktop && !f.filesOnly;
  // Mosh: a terminal session that connects directly, without forwarding.
  const mosh = shell && f.mosh;
  // A look only for sessions with a terminal.
  const terminal = shell || f.protocol === "telnet";
  const lookSize = f.lookFontSize.trim() ? Number(f.lookFontSize) : null;
  if (terminal && lookSize !== null && !(Number.isInteger(lookSize) && lookSize >= 8 && lookSize <= 32))
    return { ok: false, field: "lookFontSize", message: "Font size must be 8-32" };
  const look = {
    fontFamily: f.lookFont.trim() || null,
    fontSize: lookSize,
    terminalScheme: f.lookScheme || null,
  };
  const appearance = terminal && (look.fontFamily || look.fontSize || look.terminalScheme) ? look : null;
  if (!username && !desktop) return { ok: false, field: "username", message: "Username is required" };
  if (/\s/.test(username)) return { ok: false, field: "username", message: "No spaces allowed" };
  if (/^[-/]/.test(username)) return { ok: false, field: "username", message: "Must not start with - or /" };
  const domain = f.protocol === "rdp" ? f.domain.trim() : "";
  if (/\s/.test(domain) || /^[-/]/.test(domain))
    return { ok: false, field: "domain", message: "Not a valid domain name" };
  if (!Number.isInteger(port) || port < 1 || port > 65535)
    return { ok: false, field: "port", message: "Port must be 1-65535" };
  const name = f.name.trim() || (requireName ? "" : username ? `${username}@${host}` : host);
  if (!name) return { ok: false, field: "name", message: "Name is required" };
  if (/[\r\n]/.test(f.startupCommand))
    return { ok: false, field: "startupCommand", message: "The startup command must be one line" };
  const tags = parseTags(f.tags);
  if (tags.length > 20) return { ok: false, field: "tags", message: "At most 20 tags" };
  if (tags.some((t) => t.length > 40)) return { ok: false, field: "tags", message: "Tags are at most 40 characters" };
  let auth: SessionInput["auth"];
  if (desktop) {
    auth = { kind: "password" };
  } else if (f.authKind === "agent") {
    auth = { kind: "agent" };
  } else if (f.authKind === "keyFile") {
    const path = f.keyPath.trim();
    if (!path) return { ok: false, field: "keyPath", message: "Key file path is required" };
    if (!(path.startsWith("/") || /^[A-Za-z]:[\\/]/.test(path)))
      return { ok: false, field: "keyPath", message: "Use an absolute path" };
    auth = { kind: "keyFile", path };
  } else {
    auth = { kind: "password" };
  }
  return {
    ok: true,
    value: {
      name,
      host,
      port,
      username,
      auth,
      folder: f.folder.trim() || null,
      jumpVia: (!isFtp(f) && !mosh && f.jumpVia) || null,
      protocol: f.protocol,
      domain: domain || null,
      tags,
      forwardAgent: shell && !mosh && f.forwardAgent,
      x11Forward: shell && !mosh && f.x11Forward,
      startupCommand: (shell && f.startupCommand.trim()) || null,
      filesOnly: !desktop && f.filesOnly,
      mosh,
      appearance,
    },
  };
}
