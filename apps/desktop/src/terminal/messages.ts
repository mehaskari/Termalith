// Human-readable text for errors shown inside a terminal tab. Pure, so it
// can be unit-tested.
import { type ExitPayload, isIpcError } from "./api";

export function describeError(e: unknown): string {
  if (isIpcError(e)) return `${e.code}: ${e.message}`;
  return e instanceof Error ? e.message : String(e);
}

export function describeExit({ exit }: ExitPayload): string {
  if (!exit) return "process exited";
  if (exit.signal) return `process terminated by ${exit.signal}`;
  return `process exited with code ${exit.code}`;
}

interface ChangedDetail {
  host: string;
  port: number;
  presented: string;
  known: string[];
}

function isChangedDetail(d: unknown): d is ChangedDetail {
  if (typeof d !== "object" || d === null) return false;
  const c = d as ChangedDetail;
  return typeof c.host === "string" && typeof c.presented === "string" && Array.isArray(c.known);
}

/** Lines written to the terminal when connecting fails. */
export function connectFailureLines(e: unknown): string[] {
  if (isIpcError(e) && e.code === "host_key_changed" && isChangedDetail(e.detail)) {
    const d = e.detail;
    return [
      "WARNING: THE HOST KEY HAS CHANGED.",
      `The server ${d.host}:${d.port} presented a key that does not match the one recorded.`,
      "Someone could be intercepting this connection (man-in-the-middle),",
      "or the server's key was legitimately replaced.",
      `  presented: ${d.presented}`,
      ...d.known.map((k) => `  recorded:  ${k}`),
      "The connection was refused. Verify the new fingerprint with the server's",
      "administrator before removing the recorded key.",
    ];
  }
  if (isIpcError(e) && e.code === "host_key_rejected") {
    return ["Connection cancelled: the host key was not trusted."];
  }
  if (isIpcError(e) && e.code === "auth_failed") {
    return ["Authentication failed."];
  }
  if (isIpcError(e) && e.code === "cancelled") {
    return ["Connection cancelled."];
  }
  return [`Connection failed: ${describeError(e)}`];
}
