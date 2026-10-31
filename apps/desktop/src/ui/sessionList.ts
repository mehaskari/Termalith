import type { SessionInput, SessionRecord } from "../terminal/api";

export interface SessionGroup {
  folder: string | null;
  sessions: SessionRecord[];
}

/** Case-insensitive match on name, host, user and folder. */
export function filterSessions(sessions: SessionRecord[], query: string): SessionRecord[] {
  const q = query.trim().toLowerCase();
  if (!q) return sessions;
  return sessions.filter((s) =>
    [s.name, s.host, s.username, s.folder ?? "", `${s.username}@${s.host}`].some((f) =>
      f.toLowerCase().includes(q),
    ),
  );
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

export function authLabel(s: Pick<SessionRecord, "auth" | "hasSavedSecret">): string {
  if (s.auth.kind === "keyFile") return "key";
  if (s.auth.kind === "agent") return "agent";
  return s.hasSavedSecret ? "saved" : "";
}

export function targetLabel(s: Pick<SessionInput, "username" | "host" | "port">): string {
  return s.port === 22 ? `${s.username}@${s.host}` : `${s.username}@${s.host}:${s.port}`;
}

export interface SessionFormState {
  name: string;
  host: string;
  port: string;
  username: string;
  authKind: "password" | "keyFile" | "agent";
  keyPath: string;
  folder: string;
}

export function emptyForm(): SessionFormState {
  return { name: "", host: "", port: "22", username: "", authKind: "password", keyPath: "", folder: "" };
}

export function formFromSession(s: SessionInput): SessionFormState {
  return {
    name: s.name,
    host: s.host,
    port: String(s.port),
    username: s.username,
    authKind: s.auth.kind,
    keyPath: s.auth.kind === "keyFile" ? s.auth.path : "",
    folder: s.folder ?? "",
  };
}

/**
 * Converts the form to a SessionInput, or returns a field error. The backend
 * re-validates; this only gives faster feedback.
 */
export function parseForm(
  f: SessionFormState,
  { requireName }: { requireName: boolean },
): { ok: true; value: SessionInput } | { ok: false; field: string; message: string } {
  const host = f.host.trim();
  const username = f.username.trim();
  const port = Number(f.port);
  if (!host) return { ok: false, field: "host", message: "Host is required" };
  if (/\s/.test(host) || host.startsWith("-"))
    return { ok: false, field: "host", message: "Not a valid host name or address" };
  if (!username) return { ok: false, field: "username", message: "Username is required" };
  if (/\s/.test(username)) return { ok: false, field: "username", message: "No spaces allowed" };
  if (!Number.isInteger(port) || port < 1 || port > 65535)
    return { ok: false, field: "port", message: "Port must be 1-65535" };
  const name = f.name.trim() || (requireName ? "" : `${username}@${host}`);
  if (!name) return { ok: false, field: "name", message: "Name is required" };
  let auth: SessionInput["auth"];
  if (f.authKind === "agent") {
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
    value: { name, host, port, username, auth, folder: f.folder.trim() || null },
  };
}
