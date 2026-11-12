import { describe, expect, it } from "vitest";
import { eligibleTargets, pickableTargets, resolveTargets } from "./broadcast";

const tabs = [
  { key: 1, title: "src", terminalId: 10, kind: "ssh" },
  { key: 2, title: "web-01", terminalId: 11, kind: "ssh" },
  { key: 3, title: "web-02", terminalId: 12, kind: "ssh", ended: true },
  { key: 4, title: "connecting", kind: "ssh" },
  { key: 5, title: "tools", terminalId: 13, kind: "tools" },
  { key: 6, title: "local", terminalId: 14, kind: "local" },
];

describe("broadcast targets", () => {
  it("offers only live terminals other than the source", () => {
    expect(eligibleTargets(tabs, 1).map((t) => t.key)).toEqual([2, 6]);
  });

  it("skips targets that ended or are not connected, ignores closed tabs", () => {
    const r = resolveTargets({ source: 1, targets: [2, 3, 4, 99] }, tabs);
    expect(r.live).toEqual([{ key: 2, terminalId: 11 }]);
    expect(r.skipped.map((t) => t.key)).toEqual([3, 4]);
  });

  it("adds terminals still connecting when they belong to the group", () => {
    expect(pickableTargets(tabs, 1).map((t) => t.key)).toEqual([2, 6]);
    expect(pickableTargets(tabs, 1, [4, 5]).map((t) => t.key)).toEqual([2, 4, 6]);
  });
});
