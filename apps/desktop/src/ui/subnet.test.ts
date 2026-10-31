import { describe, expect, it } from "vitest";
import { calcSubnet } from "./subnet";

describe("calcSubnet", () => {
  it("handles CIDR notation", () => {
    expect(calcSubnet("192.168.10.77/24")).toEqual({
      network: "192.168.10.0", broadcast: "192.168.10.255", netmask: "255.255.255.0", wildcard: "0.0.0.255",
      prefix: 24, firstHost: "192.168.10.1", lastHost: "192.168.10.254", hosts: 254,
    });
  });

  it("handles dotted masks and odd prefixes", () => {
    const s = calcSubnet("10.1.2.3 255.255.240.0");
    expect(s?.prefix).toBe(20);
    expect(s?.network).toBe("10.1.0.0");
    expect(s?.broadcast).toBe("10.1.15.255");
    expect(s?.hosts).toBe(4094);
  });

  it("handles /31, /32 and /0", () => {
    expect(calcSubnet("10.0.0.1/31")).toMatchObject({ firstHost: "10.0.0.0", lastHost: "10.0.0.1", hosts: 2 });
    expect(calcSubnet("10.0.0.1/32")).toMatchObject({ network: "10.0.0.1", hosts: 1 });
    expect(calcSubnet("8.8.8.8/0")).toMatchObject({ network: "0.0.0.0", broadcast: "255.255.255.255" });
  });

  it("rejects invalid input", () => {
    for (const bad of ["", "10.0.0.1", "10.0.0.256/24", "10.0.0.1/33", "10.0.0.1 255.0.255.0", "a.b.c.d/8"]) {
      expect(calcSubnet(bad)).toBeNull();
    }
  });
});
