import { describe, expect, it } from "vitest";
import { connectFailureLines, describeExit } from "./messages";

describe("connectFailureLines", () => {
  it("explains a changed host key with both fingerprints and no accept hint", () => {
    const lines = connectFailureLines({
      code: "host_key_changed",
      message: "x",
      detail: { host: "srv", port: 22, presented: "SHA256:new", known: ["SHA256:old"] },
    });
    const text = lines.join("\n");
    expect(text).toContain("HOST KEY HAS CHANGED");
    expect(text).toContain("SHA256:new");
    expect(text).toContain("SHA256:old");
    expect(text).toContain("refused");
  });

  it("maps common codes", () => {
    expect(connectFailureLines({ code: "auth_failed", message: "" })).toEqual(["Authentication failed."]);
    expect(connectFailureLines({ code: "host_key_rejected", message: "" })[0]).toContain("not trusted");
    expect(connectFailureLines({ code: "timeout", message: "timed out" })[0]).toBe(
      "Connection failed: timeout: timed out",
    );
    expect(connectFailureLines(new Error("boom"))[0]).toBe("Connection failed: boom");
  });

  it("falls back when the changed-key detail is malformed", () => {
    expect(connectFailureLines({ code: "host_key_changed", message: "m", detail: {} })[0]).toContain(
      "host_key_changed",
    );
  });
});

describe("describeExit", () => {
  it("describes code, signal and unknown", () => {
    expect(describeExit({ exit: { code: 3, signal: null } })).toBe("process exited with code 3");
    expect(describeExit({ exit: { code: 0, signal: "SIGHUP" } })).toBe("process terminated by SIGHUP");
    expect(describeExit({ exit: null })).toBe("process exited");
  });
});
