import { describe, expect, it } from "vitest";
import { cssCursor } from "./rdpCursor";

const canvas = {} as HTMLCanvasElement;

describe("cssCursor", () => {
  it("hides the cursor when the remote pointer is hidden", () => {
    expect(cssCursor({ kind: "cursor", hidden: true, image: null }, canvas)).toBe("none");
  });

  it("uses the normal arrow without an image", () => {
    expect(cssCursor({ kind: "cursor", hidden: false, image: null }, canvas)).toBe("default");
  });

  it("refuses an image whose size does not match its pixels", () => {
    const image = { width: 2, height: 2, hotX: 0, hotY: 0, rgba: [0, 0, 0, 255] };
    expect(cssCursor({ kind: "cursor", hidden: false, image }, canvas)).toBe("default");
  });
});
