import { describe, expect, it } from "vitest";
import { DEFAULT_FONT, fontStack, resolveTheme, TERMINAL_SCHEMES, TERMINAL_THEMES, terminalTheme } from "./theme";

describe("theme", () => {
  it("resolves system preference", () => {
    expect(resolveTheme("system", true)).toBe("dark");
    expect(resolveTheme("system", false)).toBe("light");
    expect(resolveTheme("light", true)).toBe("light");
  });

  it("builds a font stack with fallbacks", () => {
    expect(fontStack("")).toBe(DEFAULT_FONT);
    expect(fontStack("JetBrains Mono")).toBe(`"JetBrains Mono", ${DEFAULT_FONT}`);
    expect(fontStack("Fira Code, monospace")).toBe(`Fira Code, monospace, ${DEFAULT_FONT}`);
  });
});

describe("terminal schemes", () => {
  it("follows the app theme by default and for unknown ids", () => {
    expect(terminalTheme("app", "dark")).toBe(TERMINAL_THEMES.dark);
    expect(terminalTheme("nope", "light")).toBe(TERMINAL_THEMES.light);
  });

  it("returns full 16-color palettes for the built-in schemes", () => {
    for (const s of TERMINAL_SCHEMES.filter((x) => x.theme)) {
      const t = terminalTheme(s.id, "dark");
      expect(t.background, s.id).toMatch(/^#[0-9a-f]{6}$/);
      expect(t.brightWhite, s.id).toMatch(/^#[0-9a-f]{6}$/);
      expect(new Set(TERMINAL_SCHEMES.map((x) => x.id)).size).toBe(TERMINAL_SCHEMES.length);
    }
  });
});
