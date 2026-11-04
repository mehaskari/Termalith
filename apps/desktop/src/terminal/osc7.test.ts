import { describe, expect, it } from "vitest";
import { FOLDER_REPORT_SETUP, parseOsc7 } from "./osc7";

describe("parseOsc7", () => {
  it("reads host and path", () => {
    expect(parseOsc7("file://web1/var/log")).toEqual({ host: "web1", path: "/var/log" });
    expect(parseOsc7("file:///tmp")).toEqual({ host: "", path: "/tmp" });
  });

  it("decodes encoded paths and keeps raw ones", () => {
    expect(parseOsc7("file://h/a%20b")?.path).toBe("/a b");
    expect(parseOsc7("file://h/a b")?.path).toBe("/a b");
    expect(parseOsc7("file://h/100%")?.path).toBe("/100%");
  });

  it("ignores anything else", () => {
    expect(parseOsc7("http://h/x")).toBeNull();
    expect(parseOsc7("file://h")).toBeNull();
    expect(parseOsc7("file://h/a%0Ab")).toBeNull();
    expect(parseOsc7("file://h/a\x07b")).toBeNull();
    expect(parseOsc7(`file://h/${"x".repeat(5000)}`)).toBeNull();
  });
});

describe("FOLDER_REPORT_SETUP", () => {
  it("is one line and stays out of history", () => {
    expect(FOLDER_REPORT_SETUP).not.toMatch(/[\r\n]/);
    expect(FOLDER_REPORT_SETUP.startsWith(" ")).toBe(true);
  });
});
