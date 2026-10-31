import { describe, expect, it } from "vitest";
import type { SessionRecord } from "../terminal/api";
import { emptyForm, filterSessions, formFromSession, groupSessions, parseForm, targetLabel } from "./sessionList";

function s(name: string, folder: string | null, host = "10.0.0.1", username = "root"): SessionRecord {
  return {
    id: `ses-${name}`,
    name,
    host,
    port: 22,
    username,
    auth: { kind: "password" },
    folder,
    hasSavedSecret: false,
    updatedAt: 0,
  };
}

describe("groupSessions", () => {
  it("sorts folders and names case-insensitively, ungrouped last", () => {
    const groups = groupSessions([s("b", null), s("Zeta", "prod"), s("alpha", "Prod"), s("a", null), s("x", "dev")]);
    expect(groups.map((g) => g.folder)).toEqual(["dev", "prod", "Prod", null]);
    expect(groups.at(-1)?.sessions.map((x) => x.name)).toEqual(["a", "b"]);
  });

  it("treats blank folders as ungrouped", () => {
    expect(groupSessions([s("a", "  ")]).map((g) => g.folder)).toEqual([null]);
  });
});

describe("filterSessions", () => {
  const all = [s("web-01", "prod", "web1.example.com", "deploy"), s("db", null, "10.0.0.9", "postgres")];
  it("matches name, host, user, folder and user@host", () => {
    expect(filterSessions(all, "WEB").map((x) => x.name)).toEqual(["web-01"]);
    expect(filterSessions(all, "10.0.0.9").map((x) => x.name)).toEqual(["db"]);
    expect(filterSessions(all, "postgres@10").map((x) => x.name)).toEqual(["db"]);
    expect(filterSessions(all, "prod").map((x) => x.name)).toEqual(["web-01"]);
    expect(filterSessions(all, "  ")).toHaveLength(2);
  });
});

describe("parseForm", () => {
  const base = { ...emptyForm(), name: "web", host: "example.com", username: "me" };

  it("accepts a valid password session", () => {
    const r = parseForm(base, { requireName: true });
    expect(r).toEqual({
      ok: true,
      value: { name: "web", host: "example.com", port: 22, username: "me", auth: { kind: "password" }, folder: null },
    });
  });

  it("derives a name for quick connect", () => {
    const r = parseForm({ ...base, name: "" }, { requireName: false });
    expect(r.ok && r.value.name).toBe("me@example.com");
    expect(parseForm({ ...base, name: "" }, { requireName: true }).ok).toBe(false);
  });

  it.each([
    [{ host: "" }, "host"],
    [{ host: "a b" }, "host"],
    [{ host: "-oProxyCommand=x" }, "host"],
    [{ username: "" }, "username"],
    [{ port: "0" }, "port"],
    [{ port: "70000" }, "port"],
    [{ port: "22.5" }, "port"],
    [{ authKind: "keyFile" as const, keyPath: "" }, "keyPath"],
    [{ authKind: "keyFile" as const, keyPath: "id_ed25519" }, "keyPath"],
  ])("rejects %j", (patch, field) => {
    const r = parseForm({ ...base, ...patch }, { requireName: true });
    expect(r.ok).toBe(false);
    if (!r.ok) expect(r.field).toBe(field);
  });

  it("accepts absolute key paths on Unix and Windows", () => {
    for (const keyPath of ["/home/me/.ssh/id_ed25519", "C:\\Users\\me\\.ssh\\id_ed25519"]) {
      const r = parseForm({ ...base, authKind: "keyFile", keyPath }, { requireName: true });
      expect(r.ok && r.value.auth).toEqual({ kind: "keyFile", path: keyPath });
    }
  });

  it("round-trips through formFromSession", () => {
    const rec = s("web", "prod");
    const r = parseForm(formFromSession(rec), { requireName: true });
    expect(r.ok && r.value).toEqual({
      name: rec.name, host: rec.host, port: rec.port, username: rec.username, auth: rec.auth, folder: rec.folder,
    });
  });
});

describe("targetLabel", () => {
  it("omits the default port", () => {
    expect(targetLabel({ username: "me", host: "h", port: 22 })).toBe("me@h");
    expect(targetLabel({ username: "me", host: "h", port: 2222 })).toBe("me@h:2222");
  });
});
