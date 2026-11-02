import { describe, expect, it } from "vitest";
import { nextIndex } from "./menuKeys";

describe("nextIndex", () => {
  it("moves and wraps", () => {
    expect(nextIndex("ArrowDown", 0, 3)).toBe(1);
    expect(nextIndex("ArrowDown", 2, 3)).toBe(0);
    expect(nextIndex("ArrowUp", 0, 3)).toBe(2);
    expect(nextIndex("Home", 2, 3)).toBe(0);
    expect(nextIndex("End", 0, 3)).toBe(2);
  });
  it("starts from the top when nothing is focused", () => {
    expect(nextIndex("ArrowDown", -1, 3)).toBe(0);
  });
  it("ignores other keys and empty menus", () => {
    expect(nextIndex("a", 0, 3)).toBeNull();
    expect(nextIndex("ArrowDown", 0, 0)).toBeNull();
  });
});
