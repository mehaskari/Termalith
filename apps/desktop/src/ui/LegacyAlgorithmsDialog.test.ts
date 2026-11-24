import { describe, expect, it } from "vitest";
import { legacyKindName } from "./LegacyAlgorithmsDialog";

describe("legacy algorithm kinds", () => {
  it("names the kinds in plain words", () => {
    expect(legacyKindName("kex")).toBe("key exchange");
    expect(legacyKindName("key")).toBe("host key");
    expect(legacyKindName("cipher")).toBe("encryption");
    expect(legacyKindName("mac")).toBe("integrity check");
    expect(legacyKindName("other")).toBe("other");
  });
});
