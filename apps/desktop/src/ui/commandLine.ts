import type { SessionProtocol } from "../terminal/api";
import { splitTarget } from "./sessionList";

/** A connection typed as a command or URL, e.g. `ssh -p 2222 me@host`. */
export interface TypedConnection {
  protocol: SessionProtocol;
  /** `mosh host` (SSH with the Mosh client). */
  mosh?: boolean;
  /** `sftp host` (SSH file browser without a terminal). */
  filesOnly?: boolean;
  user?: string;
  host: string;
  port?: number;
  /** `ssh -i key`. */
  keyPath?: string;
}

const SCHEMES: Record<string, Omit<TypedConnection, "host">> = {
  ssh: { protocol: "ssh" },
  mosh: { protocol: "ssh", mosh: true },
  sftp: { protocol: "ssh", filesOnly: true },
  telnet: { protocol: "telnet" },
  ftp: { protocol: "ftp" },
  ftps: { protocol: "ftps" },
  rdp: { protocol: "rdp" },
  vnc: { protocol: "vnc" },
};

function port(text: string | undefined): number | undefined {
  if (!text || !/^\d{1,5}$/.test(text)) return undefined;
  const n = Number(text);
  return n > 0 && n < 65536 ? n : undefined;
}

/**
 * Reads `ssh [-p port] [-l user] [-i key] [user@]host[:port]`,
 * `telnet host [port]`, `mosh`, `sftp [-P port]`, `ftp`, `ftps`, `rdp`,
 * `vnc`, or a URL such as `ssh://user@host:2222`. Returns null for anything
 * else (including unknown options), so a plain host name is left alone.
 */
export function parseTypedConnection(text: string): TypedConnection | null {
  const raw = text.trim();
  const url = /^([a-z]+):\/\/([^/\s]+)\/?$/i.exec(raw);
  if (url) {
    const scheme = SCHEMES[url[1]!.toLowerCase()];
    if (!scheme) return null;
    const t = splitTarget(decodeURIComponent(url[2]!));
    if (!t.host) return null;
    return { ...scheme, host: t.host, user: t.user, port: t.port };
  }
  const words = raw.split(/\s+/);
  const scheme = SCHEMES[(words[0] ?? "").toLowerCase()];
  if (!scheme || words.length < 2) return null;
  let user: string | undefined;
  let keyPath: string | undefined;
  let explicitPort: number | undefined;
  const rest: string[] = [];
  for (let i = 1; i < words.length; i++) {
    const w = words[i]!;
    const value = words[i + 1];
    if ((w === "-p" && scheme.protocol === "ssh" && !scheme.filesOnly) || (w === "-P" && scheme.filesOnly)) {
      explicitPort = port(value);
      if (explicitPort === undefined) return null;
      i++;
    } else if (w === "-l" && scheme.protocol === "ssh" && value) {
      user = value;
      i++;
    } else if (w === "-i" && scheme.protocol === "ssh" && !scheme.mosh && value) {
      keyPath = value;
      i++;
    } else if (w.startsWith("-")) {
      return null;
    } else {
      rest.push(w);
    }
  }
  const [target, second, ...extra] = rest;
  if (!target || extra.length > 0) return null;
  const t = splitTarget(target);
  if (!t.host) return null;
  // `telnet host 23`, `ftp host 2121`: the port as a second word.
  const secondPort = second === undefined ? undefined : port(second);
  if (second !== undefined && (secondPort === undefined || scheme.protocol === "ssh")) return null;
  return {
    ...scheme,
    host: t.host,
    user: user ?? t.user,
    port: explicitPort ?? secondPort ?? t.port,
    ...(keyPath ? { keyPath } : {}),
  };
}
