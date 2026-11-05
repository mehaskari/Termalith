import { describe, expect, it } from "vitest";
import { findKeywords } from "./highlight";

describe("findKeywords", () => {
  it("finds kinds regardless of case", () => {
    expect(findKeywords("Connection refused: ERROR")).toEqual([
      { start: 11, length: 7, kind: "bad" },
      { start: 20, length: 5, kind: "bad" },
    ]);
    expect(findKeywords("warning: disk")[0]?.kind).toBe("warn");
    expect(findKeywords("tests passed, ok")).toMatchObject([{ kind: "good" }, { kind: "good" }]);
  });

  it("matches whole words only", () => {
    expect(findKeywords("terrors bookkeeping okay failing")).toEqual([]);
    expect(findKeywords("")).toEqual([]);
  });
});
