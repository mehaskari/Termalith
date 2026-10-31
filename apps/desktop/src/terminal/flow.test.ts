import { describe, expect, it } from "vitest";
import { AckBatcher, MAX_INPUT_UNITS, SerialQueue, chunkInput } from "./flow";

describe("chunkInput", () => {
  it("returns small input as one chunk", () => {
    expect(chunkInput("ls -la\r")).toEqual(["ls -la\r"]);
  });

  it("returns nothing for empty input", () => {
    expect(chunkInput("")).toEqual([]);
  });

  it("splits at the limit and preserves content", () => {
    const data = "a".repeat(MAX_INPUT_UNITS * 2 + 5);
    const chunks = chunkInput(data);
    expect(chunks.map((c) => c.length)).toEqual([MAX_INPUT_UNITS, MAX_INPUT_UNITS, 5]);
    expect(chunks.join("")).toBe(data);
  });

  it("never splits a surrogate pair", () => {
    const data = "ab😀cd"; // 😀 is two UTF-16 units at index 2..3
    const chunks = chunkInput(data, 3);
    expect(chunks).toEqual(["ab", "😀c", "d"]);
    for (const c of chunks) expect(c.isWellFormed()).toBe(true);
  });

  it("keeps every chunk under 64 KiB of UTF-8", () => {
    const data = "ش".repeat(MAX_INPUT_UNITS + 10) + "😀".repeat(MAX_INPUT_UNITS);
    for (const c of chunkInput(data)) {
      expect(new TextEncoder().encode(c).length).toBeLessThanOrEqual(64 * 1024);
    }
  });
});

describe("AckBatcher", () => {
  function setup() {
    const sent: Array<[number, number]> = [];
    const tasks: Array<() => void> = [];
    const batcher = new AckBatcher<number>(
      (id, bytes) => sent.push([id, bytes]),
      (fn) => tasks.push(fn),
    );
    const flush = () => tasks.splice(0).forEach((t) => t());
    return { batcher, sent, flush };
  }

  it("holds bytes until the id is known, then sends them", () => {
    const { batcher, sent, flush } = setup();
    batcher.add(100);
    batcher.add(50);
    flush();
    expect(sent).toEqual([]);
    batcher.setId(7);
    flush();
    expect(sent).toEqual([[7, 150]]);
  });

  it("coalesces adds within one scheduled flush", () => {
    const { batcher, sent, flush } = setup();
    batcher.setId(1);
    batcher.add(10);
    batcher.add(20);
    batcher.add(30);
    flush();
    expect(sent).toEqual([[1, 60]]);
    batcher.add(5);
    flush();
    expect(sent).toEqual([
      [1, 60],
      [1, 5],
    ]);
  });
});

describe("SerialQueue", () => {
  it("runs tasks in order even when earlier ones are slower", async () => {
    const order: number[] = [];
    const q = new SerialQueue(() => {});
    const delay = (ms: number) => new Promise((r) => setTimeout(r, ms));
    q.push(async () => {
      await delay(20);
      order.push(1);
    });
    q.push(async () => {
      order.push(2);
    });
    await q.push(async () => {
      order.push(3);
    });
    expect(order).toEqual([1, 2, 3]);
  });

  it("continues after a failing task", async () => {
    const errors: unknown[] = [];
    const ran: string[] = [];
    const q = new SerialQueue((e) => errors.push(e));
    q.push(async () => {
      throw new Error("boom");
    });
    await q.push(async () => {
      ran.push("after");
    });
    expect(errors).toHaveLength(1);
    expect(ran).toEqual(["after"]);
  });
});
