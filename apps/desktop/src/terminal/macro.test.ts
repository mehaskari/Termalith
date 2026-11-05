import { describe, expect, it } from "vitest";
import { MacroRecorder, describeKeys, playMacro } from "./macro";

describe("MacroRecorder", () => {
  it("keeps pauses, merges quick keys and caps long waits", () => {
    const r = new MacroRecorder();
    r.add("l", 1000);
    r.add("s", 1010);
    r.add("\r", 1500);
    r.add("x", 60000);
    expect(r.steps).toEqual([
      { delayMs: 0, data: "ls" },
      { delayMs: 490, data: "\r" },
      { delayMs: 2000, data: "x" },
    ]);
  });
});

describe("describeKeys", () => {
  it("shows control keys readably", () => {
    expect(describeKeys([{ delayMs: 0, data: "ls\r\x1b[A\x03\x7f\t\x1b" }])).toBe("ls⏎↑^C⌫⇥⎋");
  });
});

describe("playMacro", () => {
  it("sends in order and stops when aborted", async () => {
    const sent: string[] = [];
    const stop = new AbortController();
    await playMacro(
      [
        { delayMs: 0, data: "a" },
        { delayMs: 1, data: "b" },
        { delayMs: 1, data: "c" },
      ],
      async (d) => {
        sent.push(d);
        if (d === "b") stop.abort();
      },
      stop.signal,
    );
    expect(sent).toEqual(["a", "b"]);
  });
});
