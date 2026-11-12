import { describe, expect, it } from "vitest";
import { parseTypedConnection } from "./commandLine";

describe("typed connections", () => {
  it("reads ssh commands with options", () => {
    expect(parseTypedConnection("ssh me@10.0.0.5")).toEqual({ protocol: "ssh", host: "10.0.0.5", user: "me", port: undefined });
    expect(parseTypedConnection("ssh -p 2222 -l root web1 ")).toEqual({ protocol: "ssh", host: "web1", user: "root", port: 2222 });
    expect(parseTypedConnection("ssh -i ~/.ssh/id_ed25519 me@h:2200")).toMatchObject({ keyPath: "~/.ssh/id_ed25519", port: 2200 });
  });

  it("reads the other protocols and URLs", () => {
    expect(parseTypedConnection("telnet 10.0.0.254 2323")).toMatchObject({ protocol: "telnet", host: "10.0.0.254", port: 2323 });
    expect(parseTypedConnection("mosh me@box")).toMatchObject({ protocol: "ssh", mosh: true, host: "box" });
    expect(parseTypedConnection("sftp -P 2022 me@files")).toMatchObject({ protocol: "ssh", filesOnly: true, port: 2022 });
    expect(parseTypedConnection("rdp://admin@win:3390")).toMatchObject({ protocol: "rdp", user: "admin", port: 3390 });
    expect(parseTypedConnection("vnc 192.168.1.40")).toMatchObject({ protocol: "vnc", host: "192.168.1.40" });
  });

  it("leaves anything else alone", () => {
    expect(parseTypedConnection("web1.example.com")).toBeNull();
    expect(parseTypedConnection("ssh")).toBeNull();
    expect(parseTypedConnection("ssh -o Foo=bar host")).toBeNull();
    expect(parseTypedConnection("ssh host extra words")).toBeNull();
    expect(parseTypedConnection("ssh host 22")).toBeNull();
    expect(parseTypedConnection("telnet host notaport")).toBeNull();
    expect(parseTypedConnection("gopher://x")).toBeNull();
  });
});
