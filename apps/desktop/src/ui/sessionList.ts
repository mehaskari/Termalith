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

export const DEFAULT_PORT: Record<SessionProtocol, number> = { ssh: 22, rdp: 3389, vnc: 5900, telnet: 23 };

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

/**
 * Nested folders (MobaXterm-style tree). Folders sort alphabetically
 * before loose sessions at each level; sessions sort by name.
 */
export function buildTree(sessions: SessionRecord[]): FolderNode {
  const root: FolderNode = { name: "", path: "", folders: [], sessions: [], total: 0 };
  const byName = (a: { name: string }, b: { name: string }) =>
    a.name.localeCompare(b.name, undefined, { sensitivity: "base" });
  for (const s of sessions) {
    let node = root;
    node.total++;
    for (const part of folderPath(s.folder)) {
      let child = node.folders.find((f) => f.name.toLowerCase() === part.toLowerCase());
      if (!child) {
        child = { name: part, path: node.path ? `${node.path}/${part}` : part, folders: [], sessions: [], total: 0 };
        node.folders.push(child);
      }
      node = child;
      node.total++;
    }
    node.sessions.push(s);
  }
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
  if (s.protocol === "rdp") return "RDP";
  if (s.protocol === "vnc") return "VNC";
  if (s.protocol === "telnet") return "Telnet";
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
      jumpVia: f.jumpVia || null,
      protocol: f.protocol,
      domain: domain || null,
      tags,
      forwardAgent: !desktop && f.forwardAgent,
      x11Forward: !desktop && f.x11Forward,
      startupCommand: (!desktop && f.startupCommand.trim()) || null,
    },
  };
}
