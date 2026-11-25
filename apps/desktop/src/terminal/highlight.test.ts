import { describe, expect, it } from "vitest";
import { findKeywords, keywordPattern } from "./highlight";

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

describe("extra keywords", () => {
  const pattern = keywordPattern({ bad: ["err-disabled", "down", "a+b"], good: ["up"] });

  it("colors the user's words whole, on top of the built-in ones", () => {
    const hits = findKeywords("Gi0/1 is down, Gi0/2 err-disabled, Gi0/3 up; error", pattern);
    expect(hits.map((h) => h.kind)).toEqual(["bad", "bad", "good", "bad"]);
    expect(findKeywords("update shutdown-x downtime", pattern)).toEqual([]);
    expect(findKeywords("DOWN", pattern)[0]?.kind).toBe("bad");
    expect(findKeywords("x a+b y", pattern)[0]?.length).toBe(3);
  });

  it("keeps the built-in words without extras", () => {
    expect(findKeywords("build failed, 2 warnings").map((h) => h.kind)).toEqual(["bad", "warn"]);
  });
});
