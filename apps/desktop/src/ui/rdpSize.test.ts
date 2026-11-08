import { describe, expect, it } from "vitest";
import { desktopSize } from "./rdpSize";

describe("desktopSize", () => {
  it("uses device pixels and scale on a Retina screen", () => {
    expect(desktopSize(1000, 700, 2)).toEqual({ width: 2000, height: 1400, scale: 200 });
  });

  it("stays at CSS pixels on a normal screen", () => {
    expect(desktopSize(1001, 701, 1)).toEqual({ width: 1002, height: 702, scale: 100 });
  });

  it("lowers the scale for a very large stage", () => {
    const s = desktopSize(1700, 1100, 2);
    expect(s.width * s.height).toBeLessThanOrEqual(2560 * 1600 + 5000);
    expect(s.scale).toBeLessThan(200);
    expect(s.scale).toBeGreaterThanOrEqual(100);
  });
});

describe("resizing", () => {
  it("lowers the scale instead of exceeding the pixel limit", () => {
    const small = desktopSize(710, 537, 2);
    const large = desktopSize(1420, 1003, small.scale / 100);
    expect(small).toEqual({ width: 1420, height: 1074, scale: 200 });
    expect(large.width * large.height).toBeLessThanOrEqual(2560 * 1600 + 5000);
    expect(large.scale).toBeLessThan(200);
  });
});
