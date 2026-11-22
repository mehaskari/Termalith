import { describe, expect, it } from "vitest";
import { moveBlocker } from "../app/tabs";
import { bufferLines } from "../terminal/TerminalView";
import { movedTerminalId } from "./MovedTerminal";

describe("moved terminal window", () => {
  it("reads only a plain terminal id from the address", () => {
    expect(movedTerminalId("?terminal=42")).toBe(42);
    expect(movedTerminalId("")).toBeNull();
    expect(movedTerminalId("?terminal=")).toBeNull();
    expect(movedTerminalId("?terminal=4x")).toBeNull();
    expect(movedTerminalId("?terminal=-1")).toBeNull();
    expect(movedTerminalId("?terminal=99999999999999999999")).toBeNull();
  });
});

describe("move to new window", () => {
  const running = { kind: "ssh" as const, terminalId: 3, ended: false, splits: [], reconnectAt: undefined };

  it("is offered for running terminals without split panes", () => {
    expect(moveBlocker(running)).toBeNull();
    expect(moveBlocker({ ...running, kind: "local" })).toBeNull();
    expect(moveBlocker({ ...running, kind: "files" })).toMatch(/Only terminal/);
    expect(moveBlocker({ ...running, terminalId: undefined })).toMatch(/not running/);
    expect(moveBlocker({ ...running, ended: true })).toMatch(/not running/);
    expect(moveBlocker({ ...running, reconnectAt: 5 })).toMatch(/not running/);
    expect(moveBlocker({ ...running, splits: [{ key: 9, open: async () => 1 }] })).toMatch(/split panes/);
  });

  it("takes the terminal's text without trailing blank lines", () => {
    const rows = ["$ ls", "a  b", "$ ", "", ""];
    const term = {
      buffer: {
        active: {
          length: rows.length,
          getLine: (i: number) => ({ translateToString: () => rows[i] ?? "" }),
        },
      },
    };
    expect(bufferLines(term as never)).toEqual(["$ ls", "a  b", "$ "]);
  });
});
