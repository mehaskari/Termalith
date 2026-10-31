import { describe, expect, it } from "vitest";
import { matchShortcut } from "./shortcuts";

const k = (key: string, mods: Partial<{ ctrl: boolean; meta: boolean; shift: boolean; alt: boolean }> = {}) => ({
  key,
  ctrlKey: !!mods.ctrl,
  metaKey: !!mods.meta,
  shiftKey: !!mods.shift,
  altKey: !!mods.alt,
});

describe("shortcuts on macOS", () => {
  it("uses Cmd", () => {
    expect(matchShortcut(k("t", { meta: true }), true)).toBe("newTab");
    expect(matchShortcut(k("w", { meta: true }), true)).toBe("closeTab");
    expect(matchShortcut(k("f", { meta: true }), true)).toBe("find");
    expect(matchShortcut(k("=", { meta: true }), true)).toBe("zoomIn");
    expect(matchShortcut(k("0", { meta: true }), true)).toBe("zoomReset");
  });

  it("leaves native copy/paste and terminal keys alone", () => {
    expect(matchShortcut(k("c", { meta: true }), true)).toBeNull();
    expect(matchShortcut(k("v", { meta: true }), true)).toBeNull();
    expect(matchShortcut(k("c", { ctrl: true }), true)).toBeNull();
  });
});

describe("shortcuts on Windows/Linux", () => {
  it("requires Ctrl+Shift so terminal programs keep Ctrl+key", () => {
    expect(matchShortcut(k("w", { ctrl: true }), false)).toBeNull();
    expect(matchShortcut(k("c", { ctrl: true }), false)).toBeNull();
    expect(matchShortcut(k("T", { ctrl: true, shift: true }), false)).toBe("newTab");
    expect(matchShortcut(k("C", { ctrl: true, shift: true }), false)).toBe("copy");
    expect(matchShortcut(k("V", { ctrl: true, shift: true }), false)).toBe("paste");
    expect(matchShortcut(k("F", { ctrl: true, shift: true }), false)).toBe("find");
    expect(matchShortcut(k("+", { ctrl: true, shift: true }), false)).toBe("zoomIn");
  });

  it("switches tabs with Ctrl+Tab everywhere", () => {
    expect(matchShortcut(k("Tab", { ctrl: true }), false)).toBe("nextTab");
    expect(matchShortcut(k("Tab", { ctrl: true, shift: true }), true)).toBe("prevTab");
  });

  it("ignores Alt combinations", () => {
    expect(matchShortcut(k("t", { ctrl: true, shift: true, alt: true }), false)).toBeNull();
  });
});
