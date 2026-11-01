import { describe, expect, it } from "vitest";
import { diffLines, sideBySide, splitLines } from "./diff";

const apply = (ops: NonNullable<ReturnType<typeof diffLines>>) => ({
  a: ops.filter((o) => o.kind !== "add").map((o) => o.text),
  b: ops.filter((o) => o.kind !== "del").map((o) => o.text),
});

describe("line diff", () => {
  it("finds a minimal edit and reproduces both sides", () => {
    const a = splitLines("listen 80\nworkers 4\ndebug false\nlog /var/log\n");
    const b = splitLines("listen 8080\nworkers 4\nlog /var/log\ntimeout 30\n");
    const ops = diffLines(a, b)!;
    expect(apply(ops)).toEqual({ a, b });
    expect(ops.filter((o) => o.kind !== "same").length).toBe(4);
  });

  it("handles identical, empty and CRLF input", () => {
    expect(diffLines(["x"], ["x"])).toEqual([{ kind: "same", text: "x" }]);
    expect(diffLines([], [])).toEqual([]);
    expect(diffLines([], ["a"])).toEqual([{ kind: "add", text: "a" }]);
    expect(splitLines("a\r\nb\r\n")).toEqual(["a", "b"]);
  });

  it("is correct on random edits", () => {
    let seed = 7;
    const rnd = () => (seed = (seed * 16807) % 2147483647) / 2147483647;
    for (let t = 0; t < 50; t++) {
      const a = Array.from({ length: Math.floor(rnd() * 30) }, () => String(Math.floor(rnd() * 6)));
      const b = a.filter(() => rnd() > 0.2).flatMap((x) => (rnd() > 0.8 ? [x, "new"] : [x]));
      expect(apply(diffLines(a, b)!)).toEqual({ a, b });
    }
  });

  it("pairs changes side by side", () => {
    const rows = sideBySide(diffLines(["a", "b", "c"], ["a", "B", "c", "d"])!);
    expect(rows.map((r) => r.kind)).toEqual(["same", "changed", "same", "add"]);
    expect(rows[1]).toMatchObject({ left: { no: 2, text: "b" }, right: { no: 2, text: "B" } });
    expect(rows[3]).toMatchObject({ left: null, right: { no: 4, text: "d" } });
  });
});
