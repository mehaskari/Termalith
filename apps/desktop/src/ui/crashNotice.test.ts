import { describe, expect, it } from "vitest";
import { issueUrl } from "./CrashNotice";

describe("issueUrl", () => {
  const report = {
    id: "crash-1-0-backend.txt",
    kind: "backend" as const,
    time: 1,
    summary: "panic at x.rs:1: boom & more",
    text: "Termalith 0.1.0 crash report\nsecret-looking backtrace",
  };

  it("prefills the title and never the report text", () => {
    const url = new URL(issueUrl(report));
    expect(url.origin + url.pathname).toBe("https://github.com/mehaskari/Termalith/issues/new");
    expect(url.searchParams.get("title")).toBe("Crash: panic at x.rs:1: boom & more");
    expect(url.searchParams.get("body")).toContain("(paste here)");
    expect(url.toString()).not.toContain("backtrace");
  });

  it("keeps titles short", () => {
    const url = new URL(issueUrl({ ...report, summary: "x".repeat(500) }));
    expect(url.searchParams.get("title")?.length).toBe(120);
  });
});
