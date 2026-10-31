import { describe, expect, it } from "vitest";
import { fillSnippet, snippetVariables, terminalInput } from "./snippetText";

describe("snippet placeholders", () => {
  it("lists distinct variables in order", () => {
    expect(snippetVariables("ping -c {{count}} {{ host }} && echo {{count}}")).toEqual(["count", "host"]);
    expect(snippetVariables("no vars {here}")).toEqual([]);
  });

  it("fills known variables and leaves unknown ones", () => {
    expect(fillSnippet("show int {{iface}} {{x}}", { iface: "Gi0/1" })).toBe("show int Gi0/1 {{x}}");
  });
});

describe("terminalInput", () => {
  it("turns newlines into Enter and only adds a final Enter when asked", () => {
    expect(terminalInput("cd /tmp\nls\n", false)).toBe("cd /tmp\rls");
    expect(terminalInput("cd /tmp\nls\n", true)).toBe("cd /tmp\rls\r");
    expect(terminalInput("uptime", true)).toBe("uptime\r");
  });
});
