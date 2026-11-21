import { describe, expect, it } from "vitest";
import type { Tab } from "../app/tabs";
import { filesTabConnection } from "./FilesTab";
import { broadcastBanner, togglePanel } from "./TerminalPaneHeader";

const tab = (key: number, title: string, terminalId?: number, ended = false) =>
  ({ key, title, terminalId, ended }) as unknown as Tab;

describe("side panels", () => {
  it("open one at a time and close again", () => {
    expect(togglePanel({}, "files")).toEqual({ snippetsOpen: false, tunnelsOpen: false, filesOpen: true });
    expect(togglePanel({ filesOpen: true }, "tunnels")).toEqual({
      snippetsOpen: false,
      tunnelsOpen: true,
      filesOpen: false,
    });
    expect(togglePanel({ snippetsOpen: true }, "snippets")).toEqual({
      snippetsOpen: false,
      tunnelsOpen: false,
      filesOpen: false,
    });
  });
});

describe("broadcast banner", () => {
  const tabs = [tab(1, "src", 10), tab(2, "web-01", 11), tab(3, "db", undefined), tab(4, "old", 12, true)];

  it("names live targets and the ones still connecting", () => {
    expect(broadcastBanner({ source: 1, targets: [2, 3, 4], tiled: false }, tabs)).toBe(
      "BROADCASTING to web-01 · waiting (not connected): db, old",
    );
    expect(broadcastBanner({ source: 1, targets: [2], tiled: false }, tabs)).toBe("BROADCASTING to web-01");
    expect(broadcastBanner({ source: 1, targets: [3], tiled: false }, tabs)).toBe(
      "BROADCASTING to no live terminals · waiting (not connected): db",
    );
  });
});

describe("files tab", () => {
  it("names the connection without the suffix", () => {
    expect(filesTabConnection("web-01 (files)")).toBe("web-01");
    expect(filesTabConnection("backups")).toBe("backups");
  });
});
