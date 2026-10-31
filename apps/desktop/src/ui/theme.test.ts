import { describe, expect, it } from "vitest";
import { DEFAULT_FONT, fontStack, resolveTheme } from "./theme";

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
