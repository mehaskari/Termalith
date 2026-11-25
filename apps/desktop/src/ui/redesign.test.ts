import { describe, expect, it } from "vitest";
import { MAX_RECENT, readRecent, rememberRecent, withRecent } from "../app/recent";
import { type Tab, linkState, sessionLinks } from "../app/tabs";
import { matchSessions } from "./HomeView";
import { SESSION_TYPES, lookInUse, optionsInUse } from "./SessionDialog";
import { emptyForm, withSessionDefaults } from "./sessionList";
import type { SessionRecord } from "../terminal/api";

const rec = (name: string, extra: Partial<SessionRecord> = {}): SessionRecord => ({
  ...withSessionDefaults({ name, host: `${name}.example.com`, port: 22, username: "admin", auth: { kind: "password" }, folder: null }),
  id: `ses-${name}`,
  hasSavedSecret: false,
  favorite: false,
  updatedAt: 0,
  shared: null,
  ...extra,
});

const tab = (patch: Partial<Tab>): Tab => ({ key: 1, title: "t", kind: "ssh", open: async () => 1, splits: [], direction: "row", ...patch }) as Tab;

describe("link lights", () => {
  it("follow the connection state of a tab", () => {
    expect(linkState(tab({ terminalId: undefined }))).toBe("wait");
    expect(linkState(tab({ terminalId: 4 }))).toBe("up");
    expect(linkState(tab({ terminalId: 4, ended: true }))).toBe("down");
    expect(linkState(tab({ terminalId: 4, ended: true, reconnectAt: 9 }))).toBe("wait");
    expect(linkState(tab({ kind: "files", desktopError: "refused" }))).toBe("down");
    expect(linkState(tab({ kind: "tools" }))).toBeNull();
    expect(linkState(tab({ kind: "home" }))).toBeNull();
    expect(linkState(tab({ kind: "desktop", desktop: null }))).toBe("wait");
    expect(linkState(tab({ kind: "desktop", desktopError: "no client" }))).toBe("down");
  });

  it("show a session's best state across its tabs", () => {
    const links = sessionLinks([
      tab({ key: 1, sessionId: "a", terminalId: 1, ended: true }),
      tab({ key: 2, sessionId: "a", terminalId: 2 }),
      tab({ key: 3, sessionId: "b", terminalId: undefined }),
      tab({ key: 4, terminalId: 5 }),
    ]);
    expect(links.get("a")).toBe("up");
    expect(links.get("b")).toBe("wait");
    expect(links.size).toBe(2);
  });
});

describe("home screen search", () => {
  const sessions = [rec("web-01", { tags: ["prod"] }), rec("db-primary", { tags: ["prod", "db"] }), rec("core-switch")];

  it("matches every typed word against name, host, user and tags", () => {
    expect(matchSessions(sessions, "prod").map((s) => s.name)).toEqual(["web-01", "db-primary"]);
    expect(matchSessions(sessions, "prod db").map((s) => s.name)).toEqual(["db-primary"]);
    expect(matchSessions(sessions, "#db").map((s) => s.name)).toEqual(["db-primary"]);
    expect(matchSessions(sessions, "SWITCH").map((s) => s.name)).toEqual(["core-switch"]);
    expect(matchSessions(sessions, "   ")).toEqual([]);
    expect(matchSessions(sessions, "nothing")).toEqual([]);
  });
});

describe("recent sessions", () => {
  it("keeps the newest first, without duplicates, at most MAX_RECENT", () => {
    expect(withRecent(["a", "b"], "b")).toEqual(["b", "a"]);
    const many = Array.from({ length: MAX_RECENT }, (_, i) => `s${i}`);
    expect(withRecent(many, "new")).toHaveLength(MAX_RECENT);
    expect(withRecent(many, "new")[0]).toBe("new");
  });

  it("survives broken or missing storage", () => {
    const store = new Map<string, string>();
    const storage = { getItem: (k: string) => store.get(k) ?? null, setItem: (k: string, v: string) => void store.set(k, v) };
    rememberRecent("x", storage);
    rememberRecent("y", storage);
    expect(readRecent(storage)).toEqual(["y", "x"]);
    store.set("termalith.recentSessions", "not json");
    expect(readRecent(storage)).toEqual([]);
    expect(readRecent(undefined)).toEqual([]);
    const throwing = { getItem: () => { throw new Error("blocked"); }, setItem: () => { throw new Error("blocked"); } };
    expect(readRecent(throwing)).toEqual([]);
    expect(() => rememberRecent("z", throwing)).not.toThrow();
  });
});

describe("session dialog", () => {
  it("offers every connection type once", () => {
    expect(SESSION_TYPES.map((t) => t.protocol)).toEqual(["ssh", "rdp", "vnc", "telnet", "ftps", "ftp", "s3"]);
  });

  it("opens a folded section only when it holds something", () => {
    expect(optionsInUse(emptyForm())).toBe(false);
    expect(optionsInUse({ ...emptyForm(), legacySsh: true })).toBe(true);
    expect(optionsInUse({ ...emptyForm(), startupCommand: "  " })).toBe(false);
    expect(lookInUse(emptyForm())).toBe(false);
    expect(lookInUse({ ...emptyForm(), icon: { name: "router", color: null } })).toBe(true);
  });
});
