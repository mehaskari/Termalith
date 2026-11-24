import { describe, expect, it } from "vitest";
import type { DesktopInfo } from "../terminal/api";
import type { Tab } from "./tabs";
import { released, withDesktopStates } from "./useTabLifecycle";
import { legacyQuestion, loginQuestion, retryReason } from "./useConnections";
import { nthTab, zoomedFontSize } from "./useShortcuts";

const desktop = (id: number, state: Partial<DesktopInfo> = {}): DesktopInfo =>
  ({
    kind: "running",
    id,
    sessionId: null,
    label: `d${id}`,
    protocol: "rdp",
    client: "client",
    address: "10.0.0.1",
    via: null,
    ...state,
  }) as DesktopInfo;

const tab = (key: number, d?: DesktopInfo): Tab =>
  ({ key, title: `t${key}`, kind: "desktop", open: { kind: "local" }, desktop: d }) as unknown as Tab;

describe("zoom shortcuts", () => {
  it("steps by one between 8 and 32 and resets to 13", () => {
    expect(zoomedFontSize(13, "zoomIn")).toBe(14);
    expect(zoomedFontSize(13, "zoomOut")).toBe(12);
    expect(zoomedFontSize(32, "zoomIn")).toBe(32);
    expect(zoomedFontSize(8, "zoomOut")).toBe(8);
    expect(zoomedFontSize(20, "zoomReset")).toBe(13);
  });
});

describe("tab number shortcuts", () => {
  it("picks the nth tab, and 9 is the last one", () => {
    const ts = ["a", "b", "c"];
    expect(nthTab(ts, 1)).toBe("a");
    expect(nthTab(ts, 3)).toBe("c");
    expect(nthTab(ts, 4)).toBeUndefined();
    expect(nthTab(ts, 9)).toBe("c");
    expect(nthTab([], 9)).toBeUndefined();
  });
});

describe("tab lifecycle", () => {
  it("lists the ids that went away", () => {
    expect(released(new Set([1, 2, 3]), new Set([2, 4]))).toEqual([1, 3]);
    expect(released(new Set<number>(), new Set([1]))).toEqual([]);
  });

  it("updates desktop states and keeps the same array when nothing changed", () => {
    const tabs = [tab(1, desktop(7)), tab(2), tab(3, desktop(8))];
    expect(withDesktopStates(tabs, [desktop(7), desktop(8)])).toBe(tabs);
    expect(withDesktopStates(tabs, [])).toBe(tabs);
    const next = withDesktopStates(tabs, [desktop(7, { kind: "exited", code: 0 } as Partial<DesktopInfo>)]);
    expect(next).not.toBe(tabs);
    expect(next[0]?.desktop?.kind).toBe("exited");
    expect(next[1]).toBe(tabs[1]);
    expect(next[2]).toBe(tabs[2]);
  });
});

describe("login questions", () => {
  const err = (code: string) => ({ code, message: code });
  const password = { keyAuth: false };
  const key = { keyAuth: true };

  it("asks for a password when the backend needs one, or after a typed one failed", () => {
    expect(loginQuestion(err("password_required"), false, password)).toBe("password");
    expect(loginQuestion(err("auth_failed"), true, password)).toBe("password");
    // A saved password or the agent failed: final, nothing typed to correct.
    expect(loginQuestion(err("auth_failed"), false, password)).toBeNull();
    expect(loginQuestion(err("host_key_rejected"), true, password)).toBeNull();
  });

  it("asks for a key passphrase only for key logins", () => {
    expect(loginQuestion(err("key_error"), false, key)).toBe("passphrase");
    expect(loginQuestion(err("key_error"), false, password)).toBeNull();
    expect(loginQuestion(err("auth_failed"), true, key)).toBeNull();
  });

  it("says why it asks again", () => {
    expect(retryReason(err("auth_failed"), true, false)).toBe("wrong");
    expect(retryReason(err("password_required"), false, true)).toBe("savedRejected");
    expect(retryReason(err("password_required"), false, false)).toBeNull();
  });
});

describe("old SSH algorithms", () => {
  it("asks only for the legacy refusal, with what the server offered", () => {
    const e = {
      code: "legacy_algorithms",
      message: "old",
      detail: { kind: "kex", offered: ["diffie-hellman-group1-sha1", 7] },
    };
    expect(legacyQuestion(e, "switch", true)).toEqual({
      target: "switch",
      kind: "kex",
      offered: ["diffie-hellman-group1-sha1"],
      canRemember: true,
    });
    expect(legacyQuestion({ code: "legacy_algorithms", message: "x" }, "q", false)).toEqual({
      target: "q",
      kind: "",
      offered: [],
      canRemember: false,
    });
    expect(legacyQuestion({ code: "no_common_algorithm", message: "x" }, "s", true)).toBeNull();
    expect(legacyQuestion({ code: "auth_failed", message: "x" }, "s", true)).toBeNull();
  });
});
