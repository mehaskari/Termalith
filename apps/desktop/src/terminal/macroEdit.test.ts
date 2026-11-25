import { describe, expect, it } from "vitest";
import { rowsToSteps } from "../ui/MacroEditDialog";
import { decodeKeys, encodeKeys, playMacro } from "./macro";

describe("macro keys as text", () => {
  it("round-trips control characters exactly", () => {
    const keys = "show run\r\tq\x1b[A\\path\x03\x7f\n";
    const text = encodeKeys(keys);
    expect(text).toBe("show run\\r\\tq\\e[A\\\\path\\x03\\x7f\\n");
    expect(decodeKeys(text)).toBe(keys);
  });

  it("leaves unknown escapes as typed", () => {
    expect(decodeKeys("a\\qb")).toBe("a\\qb");
    expect(decodeKeys("\\x4")).toBe("\\x4");
  });
});

describe("macro steps from the editor", () => {
  it("reads waits in seconds and drops empty rows", () => {
    expect(rowsToSteps([{ wait: "", keys: "ls\\r" }, { wait: "1,5", keys: "q" }, { wait: "", keys: "" }])).toEqual([
      { delayMs: 0, data: "ls\r" },
      { delayMs: 1500, data: "q" },
    ]);
  });

  it("names the first bad wait", () => {
    expect(rowsToSteps([{ wait: "1", keys: "a" }, { wait: "-2", keys: "b" }])).toEqual({ badRow: 2 });
    expect(rowsToSteps([{ wait: "abc", keys: "a" }])).toEqual({ badRow: 1 });
    expect(rowsToSteps([{ wait: "601", keys: "a" }])).toEqual({ badRow: 1 });
  });
});

describe("macro playback", () => {
  it("stops during a long wait", async () => {
    const sent: string[] = [];
    const controller = new AbortController();
    const run = playMacro(
      [
        { delayMs: 0, data: "a" },
        { delayMs: 600_000, data: "b" },
      ],
      async (d) => {
        sent.push(d);
      },
      controller.signal,
    );
    await new Promise((r) => setTimeout(r, 20));
    controller.abort();
    await run;
    expect(sent).toEqual(["a"]);
  });
});
