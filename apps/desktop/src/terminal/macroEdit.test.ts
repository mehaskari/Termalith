import { describe, expect, it } from "vitest";
import { rowsToSteps } from "../ui/MacroEditDialog";
import { decodeKeys, encodeKeys, playMacro } from "./macro";
import { plainText, publishOutput, subscribeOutput } from "./outputTap";

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

describe("waiting for text", () => {
  const fakeOutput = () => {
    let listener: ((t: string) => void) | null = null;
    return {
      output: (l: (t: string) => void) => {
        listener = l;
        return () => {
          listener = null;
        };
      },
      emit: (t: string) => listener?.(t),
      active: () => listener !== null,
    };
  };

  it("types after the prompt appears, ignoring colors", async () => {
    const out = fakeOutput();
    const sent: string[] = [];
    const run = playMacro(
      [
        { delayMs: 0, data: "enable\r" },
        { delayMs: 0, data: "show run\r", waitFor: "switch#" },
      ],
      async (d) => {
        sent.push(d);
      },
      new AbortController().signal,
      { output: out.output, timeoutMs: 2000 },
    );
    await new Promise((r) => setTimeout(r, 20));
    expect(sent).toEqual(["enable\r"]);
    out.emit("Password: \r\n\x1b[1mswitch\x1b[0m#");
    await run;
    expect(sent).toEqual(["enable\r", "show run\r"]);
    expect(out.active()).toBe(false);
  });

  it("stops with an error when the text does not come", async () => {
    const out = fakeOutput();
    await expect(
      playMacro([{ delayMs: 0, data: "x", waitFor: "never" }], async () => {}, new AbortController().signal, {
        output: out.output,
        timeoutMs: 30,
      }),
    ).rejects.toThrow(/"never" did not appear/);
  });

  it("reads the wait text from the editor", () => {
    expect(rowsToSteps([{ wait: "", waitFor: " # ", keys: "q" }])).toEqual([{ delayMs: 0, data: "q", waitFor: " # " }]);
    expect(rowsToSteps([{ wait: "", waitFor: "  ", keys: "q" }])).toEqual([{ delayMs: 0, data: "q" }]);
  });
});

describe("output tap", () => {
  it("decodes only for listeners and strips escape sequences", () => {
    const got: string[] = [];
    publishOutput(9, new TextEncoder().encode("before"));
    const off = subscribeOutput(9, (t) => got.push(t));
    const bytes = new TextEncoder().encode("é");
    publishOutput(9, bytes.slice(0, 1));
    publishOutput(9, bytes.slice(1));
    off();
    publishOutput(9, new TextEncoder().encode("after"));
    expect(got.join("")).toBe("é");
    expect(plainText("\x1b[32mok\x1b[0m \x1b]0;title\x07done")).toBe("ok done");
  });
});
