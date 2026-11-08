import { describe, expect, it } from "vitest";
import { parseFrame } from "../terminal/api";
import { scancode } from "./scancodes";

describe("scancode", () => {
  it("maps letters, digits and modifiers", () => {
    expect(scancode("KeyA")).toBe(0x1e);
    expect(scancode("Digit1")).toBe(0x02);
    expect(scancode("ShiftLeft")).toBe(0x2a);
    expect(scancode("Enter")).toBe(0x1c);
  });
  it("marks extended keys", () => {
    expect(scancode("ArrowLeft")).toBe(0xe04b);
    expect(scancode("NumpadEnter")).toBe(0xe01c);
    expect(scancode("MetaLeft")).toBe(0xe05b);
  });
  it("returns null for keys without a scan code", () => {
    expect(scancode("Fn")).toBeNull();
    expect(scancode("")).toBeNull();
  });
});

describe("parseFrame", () => {
  it("reads the header and pixels", () => {
    const buf = new ArrayBuffer(12 + 2 * 1 * 4);
    const v = new DataView(buf);
    [3, 4, 2, 1, 1920, 1080].forEach((n, i) => v.setUint16(i * 2, n, true));
    const f = parseFrame(buf);
    expect(f && [f.x, f.y, f.w, f.h, f.desktopWidth, f.desktopHeight, f.rgba.length]).toEqual([3, 4, 2, 1, 1920, 1080, 8]);
  });
  it("rejects a length that does not match the size", () => {
    const buf = new ArrayBuffer(12 + 3);
    new DataView(buf).setUint16(4, 1, true);
    expect(parseFrame(buf)).toBeNull();
  });
});
