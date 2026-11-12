import { describe, expect, it } from "vitest";
import { pasteWarnings, revealHidden } from "./pasteCheck";

describe("paste warnings", () => {
  it("lets a plain one-line command through", () => {
    expect(pasteWarnings("ls -la /var/log")).toEqual([]);
  });

  it("warns about several lines and a trailing line break", () => {
    expect(pasteWarnings("apt update\napt upgrade -y\n")[0]).toMatch(/^2 lines/);
    expect(pasteWarnings("reboot\r\n")[0]).toMatch(/runs at once/);
  });

  it("warns about piping a download into a shell", () => {
    expect(pasteWarnings("curl -fsSL https://x.example/i.sh | sudo bash")).toContain("it downloads a script and runs it");
    expect(pasteWarnings("wget -qO- x | sh")).toContain("it downloads a script and runs it");
    expect(pasteWarnings("curl https://x.example -o file")).toEqual([]);
  });

  it("warns about invisible and control characters and shows them", () => {
    const hidden = "echo hi​";
    expect(pasteWarnings(hidden).join()).toMatch(/invisible/);
    expect(pasteWarnings("ok\x1b[31m").join()).toMatch(/control/);
    expect(revealHidden(hidden)).toBe("echo hi⟨U+200B⟩");
  });
});
