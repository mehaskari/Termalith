import { describe, expect, it } from "vitest";
import { formatSize, joinRemote, parentRemote, percent } from "./paths";

describe("remote paths", () => {
  it("joins with exactly one slash", () => {
    expect(joinRemote("/home/me", "a")).toBe("/home/me/a");
    expect(joinRemote("/", "a")).toBe("/a");
  });

  it("finds the parent", () => {
    expect(parentRemote("/home/me")).toBe("/home");
    expect(parentRemote("/home/me/")).toBe("/home");
    expect(parentRemote("/home")).toBe("/");
    expect(parentRemote("/")).toBe("/");
    expect(parentRemote("/C:/Users")).toBe("/C:/");
  });
});

describe("formatSize", () => {
  it("uses binary units", () => {
    expect(formatSize(null)).toBe("");
    expect(formatSize(512)).toBe("512 B");
    expect(formatSize(1536)).toBe("1.5 KB");
    expect(formatSize(20 * 1024 * 1024)).toBe("20 MB");
    expect(formatSize(5 * 1024 ** 4)).toBe("5.0 TB");
  });
});

describe("percent", () => {
  it("is null without a total and clamps at 100", () => {
    expect(percent(5, null)).toBeNull();
    expect(percent(5, 10)).toBe(50);
    expect(percent(20, 10)).toBe(100);
  });
});
