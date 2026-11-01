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

describe("tab numbers and palette", () => {
  const k = (key: string, o: Partial<{ ctrlKey: boolean; metaKey: boolean; shiftKey: boolean; code: string }> = {}) => ({
    key,
    ctrlKey: false,
    metaKey: false,
    shiftKey: false,
    altKey: false,
    ...o,
  });
  it("maps Cmd+1..9 on macOS and Ctrl+Shift+1..9 elsewhere (by physical key)", () => {
    expect(matchShortcut(k("3", { metaKey: true, code: "Digit3" }), true)).toBe("tab3");
    expect(matchShortcut(k("#", { ctrlKey: true, shiftKey: true, code: "Digit3" }), false)).toBe("tab3");
    expect(matchShortcut(k("3", { ctrlKey: true, code: "Digit3" }), false)).toBe(null);
  });
  it("opens the palette with Cmd+K / Ctrl+Shift+K", () => {
    expect(matchShortcut(k("k", { metaKey: true }), true)).toBe("palette");
    expect(matchShortcut(k("K", { ctrlKey: true, shiftKey: true }), false)).toBe("palette");
  });
});
