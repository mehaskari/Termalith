import { describe, expect, it } from "vitest";
import type { SessionRecord } from "../terminal/api";
import {
  authLabel,
  emptyForm,
  filterSessions,
  formFromSession,
  groupSessions,
  parseForm,
  parseTags,
  splitTarget,
  allFolderPaths,
  buildTree,
  folderDropPath,
  folderPath,
  isInside,
  joinFolder,
  parentFolder,
  allSessions,
  targetLabel,
  withProtocol,
} from "./sessionList";

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
      value: { name: "web", host: "example.com", port: 22, username: "me", auth: { kind: "password" }, folder: null, jumpVia: null, protocol: "ssh", domain: null, tags: [], forwardAgent: false, x11Forward: false, startupCommand: null },
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
      name: rec.name, host: rec.host, port: rec.port, username: rec.username, auth: rec.auth, folder: rec.folder, jumpVia: null, protocol: "ssh", domain: null, tags: [], forwardAgent: false, x11Forward: false, startupCommand: null,
    });
  });
});

describe("targetLabel", () => {
  it("omits the default port", () => {
    expect(targetLabel({ username: "me", host: "h", port: 22 })).toBe("me@h");
    expect(targetLabel({ username: "me", host: "h", port: 2222 })).toBe("me@h:2222");
  });
});

describe("agent auth", () => {
  it("parses and labels agent sessions", () => {
    const r = parseForm({ ...emptyForm(), name: "n", host: "h", username: "u", authKind: "agent" }, { requireName: true });
    expect(r.ok && r.value.auth).toEqual({ kind: "agent" });
    expect(authLabel({ auth: { kind: "agent" }, hasSavedSecret: false })).toBe("agent");
    expect(authLabel({ auth: { kind: "password" }, hasSavedSecret: true })).toBe("saved");
    expect(authLabel({ auth: { kind: "password" }, hasSavedSecret: false })).toBe("");
  });
});

describe("jump host", () => {
  it("round-trips the jump session id", () => {
    const r = parseForm({ ...emptyForm(), name: "n", host: "h", username: "u", jumpVia: "ses-abc" }, { requireName: true });
    expect(r.ok && r.value.jumpVia).toBe("ses-abc");
  });
});

describe("remote desktop sessions", () => {
  it("moves the default port with the protocol and keeps custom ports", () => {
    const f = withProtocol(emptyForm(), "rdp");
    expect(f.port).toBe("3389");
    expect(withProtocol(f, "vnc").port).toBe("5900");
    expect(withProtocol({ ...f, port: "13389" }, "vnc").port).toBe("13389");
  });

  it("parses RDP without a user name, forces password auth, keeps the domain", () => {
    const f = { ...withProtocol(emptyForm(), "rdp"), name: "win", host: "win.local", domain: "CORP", authKind: "agent" as const };
    const r = parseForm(f, { requireName: true });
    expect(r.ok && r.value).toMatchObject({ protocol: "rdp", username: "", auth: { kind: "password" }, domain: "CORP", port: 3389 });
    const vnc = parseForm({ ...f, protocol: "vnc" }, { requireName: true });
    expect(vnc.ok && vnc.value.domain).toBe(null);
  });

  it("rejects option-like user names and labels targets", () => {
    const f = { ...withProtocol(emptyForm(), "rdp"), name: "w", host: "h", username: "/p:x" };
    expect(parseForm(f, { requireName: true })).toMatchObject({ ok: false, field: "username" });
    expect(targetLabel({ username: "", host: "h", port: 3389, protocol: "rdp" })).toBe("h");
    expect(targetLabel({ username: "u", host: "h", port: 5901, protocol: "vnc" })).toBe("u@h:5901");
    expect(authLabel({ auth: { kind: "password" }, hasSavedSecret: false, protocol: "vnc" })).toBe("VNC");
  });
});

describe("tags", () => {
  const tagged = (name: string, tags: string[]): SessionRecord => ({ ...s(name, null), tags });

  it("filters by #tag prefix and by plain words in tags", () => {
    const list = [tagged("a", ["prod", "web"]), tagged("b", ["staging"]), tagged("c", [])];
    expect(filterSessions(list, "#pro").map((x) => x.name)).toEqual(["a"]);
    expect(filterSessions(list, "staging").map((x) => x.name)).toEqual(["b"]);
    expect(filterSessions(list, "#prod a").map((x) => x.name)).toEqual(["a"]);
    expect(filterSessions(list, "#prod zzz")).toEqual([]);
    expect(filterSessions(list, "#").length).toBe(0);
  });

  it("parses, dedupes and collects tags", () => {
    expect(parseTags(" prod, #web ,, PROD,db ")).toEqual(["prod", "web", "db"]);
    const f = { ...emptyForm(), name: "n", host: "h", username: "u", tags: "x, y" };
    const r = parseForm(f, { requireName: true });
    expect(r.ok && r.value.tags).toEqual(["x", "y"]);
    expect(formFromSession({ ...s("t", null), tags: ["x", "y"] }).tags).toBe("x, y");
  });
});

describe("agent forwarding", () => {
  it("is kept for SSH and dropped for remote desktop sessions", () => {
    const f = { ...emptyForm(), name: "n", host: "h", username: "u", forwardAgent: true };
    const r = parseForm(f, { requireName: true });
    expect(r.ok && r.value.forwardAgent).toBe(true);
    const rdp = parseForm({ ...withProtocol(f, "rdp"), username: "" }, { requireName: true });
    expect(rdp.ok && rdp.value.forwardAgent).toBe(false);
  });
});

describe("startup command", () => {
  it("is trimmed, optional and one line", () => {
    const f = { ...emptyForm(), name: "n", host: "h", username: "u", startupCommand: "  cd /srv  " };
    const r = parseForm(f, { requireName: true });
    expect(r.ok && r.value.startupCommand).toBe("cd /srv");
    expect(parseForm({ ...f, startupCommand: "" }, { requireName: true })).toMatchObject({ ok: true, value: { startupCommand: null } });
    expect(parseForm({ ...f, startupCommand: "a\nb" }, { requireName: true })).toMatchObject({ ok: false, field: "startupCommand" });
  });
});

describe("telnet sessions", () => {
  it("defaults to port 23, allows no user and keeps the jump host", () => {
    const f = { ...withProtocol(emptyForm(), "telnet"), name: "switch", host: "10.0.0.254", jumpVia: "ses-b" };
    expect(f.port).toBe("23");
    const r = parseForm(f, { requireName: true });
    expect(r.ok && r.value).toMatchObject({ protocol: "telnet", username: "", port: 23, jumpVia: "ses-b", auth: { kind: "password" } });
    expect(r.ok && r.value.forwardAgent).toBe(false);
    expect(authLabel({ auth: { kind: "password" }, hasSavedSecret: false, protocol: "telnet" })).toBe("Telnet");
    expect(targetLabel({ username: "", host: "10.0.0.254", port: 23, protocol: "telnet" })).toBe("10.0.0.254");
  });
});

describe("user@host:port in the host field", () => {
  it("splits targets", () => {
    expect(splitTarget("root@10.0.0.1:2222")).toEqual({ user: "root", host: "10.0.0.1", port: 2222 });
    expect(splitTarget("example.com")).toEqual({ user: undefined, host: "example.com" });
    expect(splitTarget("fe80::1")).toEqual({ user: undefined, host: "fe80::1" });
    expect(splitTarget("me@[fe80::1]:22")).toEqual({ user: "me", host: "fe80::1", port: 22 });
  });
  it("fills user and port unless they were set explicitly", () => {
    const f = { ...emptyForm(), host: "admin@sw1:2200" };
    const r = parseForm(f, { requireName: false });
    expect(r.ok && r.value).toMatchObject({ host: "sw1", username: "admin", port: 2200, name: "admin@sw1" });
    const explicit = parseForm({ ...f, username: "ops", port: "2022" }, { requireName: false });
    expect(explicit.ok && explicit.value).toMatchObject({ username: "ops", port: 2022 });
  });
});

describe("command palette filter", () => {
  it("matches every word in label, detail or group", async () => {
    const { filterPalette } = await import("./CommandPalette");
    const items = [
      { id: "1", label: "web-01", detail: "deploy@web1", group: "Sessions" as const, run: () => {} },
      { id: "2", label: "Settings", group: "Actions" as const, run: () => {} },
    ];
    expect(filterPalette(items, "web deploy").map((i) => i.id)).toEqual(["1"]);
    expect(filterPalette(items, "actions set").map((i) => i.id)).toEqual(["2"]);
    expect(filterPalette(items, "").length).toBe(2);
  });
});

describe("folder tree", () => {
  it("nests folders by / and counts sessions", () => {
    const list = [s("db1", "Production / DB"), s("web", "Production"), s("db2", "production/db"), s("loose", null), s("lab", "Lab")];
    const t = buildTree(list);
    expect(t.total).toBe(5);
    expect(t.folders.map((f) => f.name)).toEqual(["Lab", "Production"]);
    const prod = t.folders[1]!;
    expect(prod.total).toBe(3);
    expect(prod.sessions.map((x) => x.name)).toEqual(["web"]);
    expect(prod.folders[0]!.path).toBe("Production/DB");
    expect(prod.folders[0]!.sessions.map((x) => x.name)).toEqual(["db1", "db2"]);
    expect(t.sessions.map((x) => x.name)).toEqual(["loose"]);
    expect(allSessions(prod).map((x) => x.name).sort()).toEqual(["db1", "db2", "web"]);
    expect(folderPath(" a / /b ")).toEqual(["a", "b"]);
  });

  it("adds empty stored folders without counting them", () => {
    const t = buildTree([s("web", "Prod")], ["Prod/Empty", "Lab / Switches"]);
    expect(t.folders.map((f) => f.name)).toEqual(["Lab", "Prod"]);
    expect(t.folders[0]!.folders[0]!.path).toBe("Lab/Switches");
    expect(t.folders[0]!.total).toBe(0);
    expect(t.folders[1]!.folders.map((f) => f.name)).toEqual(["Empty"]);
    expect(t.folders[1]!.total).toBe(1);
  });

  it("works out folder paths and moves", () => {
    expect(parentFolder("A/B/C")).toBe("A/B");
    expect(parentFolder("A")).toBe("");
    expect(joinFolder("", " x ")).toBe("x");
    expect(joinFolder("A / B", "C")).toBe("A/B/C");
    expect(isInside("Prod/DB", "prod")).toBe(true);
    expect(isInside("Production", "Prod")).toBe(false);
    expect(folderDropPath("Prod/DB", "")).toBe("DB");
    expect(folderDropPath("Prod/DB", "Lab")).toBe("Lab/DB");
    expect(folderDropPath("Prod", "Prod/DB")).toBeNull();
    expect(folderDropPath("Prod", "prod")).toBeNull();
    expect(folderDropPath("Prod/DB", "Prod")).toBeNull();
    expect(folderDropPath("Lab", "")).toBeNull();
    expect(allFolderPaths([s("a", "Prod / DB"), s("b", null)], ["lab/x", "prod"])).toEqual(["lab", "lab/x", "prod", "prod/DB"]);
  });
});
