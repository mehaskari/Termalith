import { describe, expect, it } from "vitest";
import { formatBytes } from "./monitorFormat";

describe("formatBytes", () => {
  it("picks a unit", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(1023)).toBe("1023 B");
    expect(formatBytes(1536)).toBe("2 KB");
    expect(formatBytes(5 * 1024 ** 2)).toBe("5 MB");
    expect(formatBytes(1.5 * 1024 ** 3)).toBe("1.5 GB");
    expect(formatBytes(2 * 1024 ** 5)).toBe("2.0 PB");
  });
});
