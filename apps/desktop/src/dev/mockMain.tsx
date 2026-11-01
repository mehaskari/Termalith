// Development only: renders the real UI against a simulated backend so the
// layout and dialog flows can be checked in a normal browser. Loaded from
// mock.html, which the production build does not include.
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { emit } from "@tauri-apps/api/event";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { App } from "../App";
import type { HostKeyAnswer, SessionInput, SessionRecord } from "../terminal/api";
import "../styles.css";

interface ChannelLike {
  id: number;
}

const enc = new TextEncoder();
const sessions = new Map<string, SessionRecord>();
const outputs = new Map<number, { ch: ChannelLike; index: number }>();
const hostKeyWaiters = new Map<number, (a: HostKeyAnswer) => void>();
const trustedHosts = new Set<string>();
const forgotten = new Set<string>();
const remoteFiles: Record<string, Array<{ name: string; kind: string; size: number | null }>> = {
  "/home/deploy": [
    { name: "releases", kind: "dir", size: null },
    { name: "logs", kind: "dir", size: null },
    { name: "app.env", kind: "file", size: 312 },
    { name: "backup-2026-10-30.tar.gz", kind: "file", size: 48_234_112 },
  ],
  "/home/deploy/logs": [{ name: "app.log", kind: "file", size: 1_204_331 }],
  "/home/deploy/releases": [],
};
let nextTransfer = 1;
const mockSnippets: Array<{ id: string; name: string; command: string; folder: string | null }> = [
  { id: "snp-1", name: "Interface status", command: "show interfaces {{iface}} status", folder: "Cisco" },
  { id: "snp-2", name: "Disk usage", command: "df -h\ndu -sh {{path}}", folder: null },
];
const mockSettings: Record<string, unknown> = { theme: "system", fontSize: 13, fontFamily: "", cursorStyle: "block", cursorBlink: true, scrollback: 5000, logSessions: false };
const logging = new Set<number>();
let nextTunnel = 1;
const tunnels: Array<Record<string, unknown> & { id: number; terminalId: number }> = [];
function progressThen(ch: ChannelLike, total: number, done: unknown) {
  let index = 0;
  let sent = 0;
  const step = () => {
    sent = Math.min(total, sent + total / 8);
    window.__TAURI_INTERNALS__.runCallback(ch.id, { index: index++, message: { kind: "progress", transferred: sent, total } });
    if (sent < total) setTimeout(step, 150);
    else window.__TAURI_INTERNALS__.runCallback(ch.id, { index: index++, message: done });
  };
  setTimeout(step, 100);
}
let nextTerm = 1;
let nextPrompt = 1;
let nextSession = 1;

function send(id: number, text: string) {
  const o = outputs.get(id);
  if (!o) return;
  const buf = enc.encode(text).buffer;
  window.__TAURI_INTERNALS__.runCallback(o.ch.id, { index: o.index++, message: buf });
}

function seed(input: SessionInput, hasSavedSecret = false) {
  const id = `ses-${String(nextSession++).padStart(24, "0")}`;
  sessions.set(id, { ...input, id, hasSavedSecret, updatedAt: 0 });
}

seed({ name: "web-01", host: "web1.example.com", port: 22, username: "deploy", auth: { kind: "password" }, folder: "Production" });
seed({ name: "db-primary", host: "10.0.0.9", port: 2222, username: "postgres", auth: { kind: "keyFile", path: "/Users/me/.ssh/id_ed25519" }, folder: "Production" });
seed({ name: "lab router", host: "192.168.1.1", port: 22, username: "admin", auth: { kind: "password" }, folder: null }, true);

seed({ name: "win-jump", host: "10.0.5.20", port: 3389, username: "", auth: { kind: "password" }, folder: "Production", jumpVia: "ses-000000000000000000000001", protocol: "rdp", domain: "CORP" });
seed({ name: "lab vnc", host: "192.168.1.40", port: 5900, username: "", auth: { kind: "password" }, folder: null, protocol: "vnc" });

const desktops = new Map<number, Record<string, unknown>>();
let nextDesktop = 1;

declare global {
  interface Window {
    __TAURI_INTERNALS__: { runCallback: (id: number, data: unknown) => void };
  }
}

mockWindows("main");
mockIPC(
  async (cmd, args) => {
    const a = (args ?? {}) as Record<string, unknown>;
    switch (cmd) {
      case "app_info":
        return { version: "0.0.0-mock", secretsAvailable: true, agentAvailable: true };
      case "settings_get":
        return mockSettings;
      case "settings_save":
        Object.assign(mockSettings, a.settings);
        return null;
      case "session_set_favorite": {
        const s = sessions.get(a.id as string);
        if (s) s.favorite = a.favorite as boolean;
        return null;
      }
      case "sessions_export":
        return { json: JSON.stringify({ format: "termalith-sessions", version: 1, sessions: [...sessions.values()].map(({ id: _i, hasSavedSecret: _h, updatedAt: _u, ...rest }) => rest) }, null, 2), savedTo: "/Users/me/Downloads/termalith-sessions.json" };
      case "sessions_import":
        return { created: [], skipped: [], unresolvedJumps: [] };
      case "ssh_config_preview":
        return [
          { alias: "prod-web", input: { name: "prod-web", host: "prod-web.example.com", port: 22, username: "deploy", auth: { kind: "agent" }, folder: "SSH config" }, warnings: [], jumpAlias: null },
          { alias: "db", input: { name: "db", host: "10.0.0.9", port: 2222, username: "postgres", auth: { kind: "keyFile", path: "/Users/me/.ssh/id_db" }, folder: "SSH config" }, warnings: [], jumpAlias: null },
          { alias: "bastion", input: { name: "bastion", host: "bastion.example.com", port: 22, username: "me", auth: { kind: "agent" }, folder: "SSH config" }, warnings: [], jumpAlias: null },
          { alias: "app-internal", input: { name: "app-internal", host: "10.1.1.5", port: 22, username: "me", auth: { kind: "agent" }, folder: "SSH config" }, warnings: [], jumpAlias: "bastion" },
        ];
      case "ssh_config_import": {
        const created: string[] = [];
        for (const alias of a.aliases as string[]) {
          seed({ name: alias, host: `${alias}.example.com`, port: 22, username: "me", auth: { kind: "agent" }, folder: "SSH config" });
          created.push(alias);
        }
        return { created, skipped: [], unresolvedJumps: [] };
      }
      case "terminal_log_start":
        logging.add(Number(a.id));
        return { path: `/Users/me/Documents/Termalith Logs/2026-11-01_10-00-00_${String(a.label)}.log`, bytes: 0, dropped: 0 };
      case "terminal_log_stop":
        logging.delete(Number(a.id));
        return null;
      case "desktop_clients":
        return { rdp: { name: "Microsoft Remote Desktop", asksPasswordFirst: false }, vnc: { name: "Screen Sharing", asksPasswordFirst: false } };
      case "desktop_open": {
        const s = sessions.get(String(a.sessionId));
        if (!s) throw { code: "not_found", message: "not found" };
        const jump = s.jumpVia ? sessions.get(s.jumpVia) : undefined;
        const d = {
          id: nextDesktop++,
          sessionId: s.id,
          label: s.name,
          protocol: s.protocol,
          client: s.protocol === "rdp" ? "Microsoft Remote Desktop" : "Screen Sharing",
          address: jump ? "127.0.0.1:52144" : `${s.host}:${s.port}`,
          via: jump?.name ?? null,
          kind: "handedOff",
        };
        desktops.set(d.id, d);
        return d;
      }
      case "desktop_list":
        return [...desktops.values()];
      case "desktop_close":
        desktops.delete(Number(a.id));
        return null;
      case "sessions_list":
        return [...sessions.values()];
      case "session_create": {
        seed(a.input as SessionInput);
        return [...sessions.values()].at(-1);
      }
      case "session_update": {
        const old = sessions.get(a.id as string);
        if (!old) throw { code: "not_found", message: "not found" };
        const rec = { ...old, ...(a.input as SessionInput) };
        sessions.set(rec.id, rec);
        return rec;
      }
      case "session_delete":
        sessions.delete(a.id as string);
        return null;
      case "session_forget_secret": {
        const s = sessions.get(a.id as string);
        if (s) s.hasSavedSecret = false;
        return null;
      }
      case "snippets_list":
        return mockSnippets;
      case "snippet_create": {
        const s = { id: `snp-${mockSnippets.length + 1}`, ...(a.input as object) } as (typeof mockSnippets)[number];
        mockSnippets.push(s);
        return s;
      }
      case "snippet_update": {
        const i = mockSnippets.findIndex((s) => s.id === a.id);
        mockSnippets[i] = { id: a.id as string, ...(a.input as object) } as (typeof mockSnippets)[number];
        return mockSnippets[i];
      }
      case "snippet_delete":
        mockSnippets.splice(mockSnippets.findIndex((s) => s.id === a.id), 1);
        return null;
      case "net_tcp_check":
        return { open: a.port === 22, address: a.port === 22 ? `${a.host}:22` : null, millis: 12, error: a.port === 22 ? null : "Connection refused" };
      case "net_dns_lookup":
        return ["93.184.216.34", "2606:2800:220:1::248"];
      case "net_probe_start": {
        const ch = a.onLine as ChannelLike;
        const done = a.onDone as ChannelLike;
        let i = 0;
        const tick = () => {
          window.__TAURI_INTERNALS__.runCallback(ch.id, { index: i, message: `64 bytes from ${a.host}: icmp_seq=${i} ttl=56 time=1${i}.2 ms` });
          i++;
          if (i < 4) setTimeout(tick, 300);
          else window.__TAURI_INTERNALS__.runCallback(done.id, { index: 0, message: 0 });
        };
        setTimeout(tick, 100);
        return 1;
      }
      case "net_probe_cancel":
        return true;
      case "serial_ports":
        return [{ path: "/dev/cu.usbserial-A10K", description: "USB (FTDI FT232R USB UART)" }, { path: "/dev/cu.Bluetooth-Incoming-Port", description: "" }];
      case "serial_open": {
        const id = nextTerm++;
        outputs.set(id, { ch: a.onOutput as ChannelLike, index: 0 });
        setTimeout(() => send(id, "\r\nSwitch>"), 50);
        return id;
      }
      case "terminal_split": {
        const id = nextTerm++;
        outputs.set(id, { ch: a.onOutput as ChannelLike, index: 0 });
        setTimeout(() => send(id, `\x1b[2m(split of terminal ${a.source})\x1b[0m\r\n$ `), 50);
        return id;
      }
      case "terminal_open": {
        const id = nextTerm++;
        outputs.set(id, { ch: a.onOutput as ChannelLike, index: 0 });
        setTimeout(() => send(id, "\x1b[2m(mock local shell)\x1b[0m\r\n$ "), 50);
        return id;
      }
      case "ssh_open": {
        const req = a.request as { sessionId?: string; quick?: SessionInput; password?: string; savePassword?: boolean };
        const target = req.sessionId ? sessions.get(req.sessionId) : (req.quick as SessionRecord | undefined);
        if (!target) throw { code: "not_found", message: "not found" };
        const hostKey = `${target.host}:${target.port}`;
        if (target.host === "192.168.1.1" && !forgotten.has(hostKey)) {
          throw {
            code: "host_key_changed",
            message: "HOST KEY CHANGED",
            detail: { host: target.host, port: target.port, presented: "SHA256:NEWkeyNEWkeyNEWkey", known: ["SHA256:OLDkeyOLDkeyOLDkey"] },
          };
        }
        if (!trustedHosts.has(hostKey)) {
          const requestId = nextPrompt++;
          const answer = await new Promise<HostKeyAnswer>((resolve) => {
            hostKeyWaiters.set(requestId, resolve);
            void emit("host-key-prompt", {
              requestId,
              host: target.host,
              port: target.port,
              algorithm: "ssh-ed25519",
              fingerprint: "SHA256:k3mOz2Lr9cBgQ0uQ8w1hQmYwF3x7mTq5r6bXo2ZpE7s",
            });
          });
          if (answer === "reject") throw { code: "host_key_rejected", message: "host key was not trusted" };
          if (answer === "acceptAndSave") trustedHosts.add(hostKey);
        }
        if (target.auth.kind === "password") {
          const saved = "hasSavedSecret" in target && target.hasSavedSecret;
          if (!req.password && !saved) throw { code: "password_required", message: "a password is required" };
          if (req.password && req.password !== "secret") throw { code: "auth_failed", message: "authentication failed" };
          if (req.savePassword && req.sessionId) {
            const s = sessions.get(req.sessionId);
            if (s) s.hasSavedSecret = true;
          }
        }
        const id = nextTerm++;
        outputs.set(id, { ch: a.onOutput as ChannelLike, index: 0 });
        setTimeout(() => send(id, `Connected to ${target.host} (mock)\r\n${target.username}@${target.host}:~$ `), 50);
        return { id, label: target.name || `${target.username}@${target.host}` };
      }
      case "host_key_respond": {
        const w = hostKeyWaiters.get(a.requestId as number);
        hostKeyWaiters.delete(a.requestId as number);
        w?.(a.answer as HostKeyAnswer);
        return w !== undefined;
      }
      case "sftp_home":
        return "/home/deploy";
      case "sftp_list": {
        const list = remoteFiles[a.path as string];
        if (!list) throw { code: "sftp_error", message: `No such directory: ${a.path}` };
        return list.map((e) => ({ ...e, modified: 1_790_000_000, permissions: 0o644 }));
      }
      case "sftp_download": {
        const size = (a.size as number | null) ?? 1000;
        progressThen(a.onEvent as ChannelLike, size, { kind: "done", destination: `/Users/me/Downloads/${a.name}` });
        return { id: nextTransfer++, downloadsDir: "/Users/me/Downloads" };
      }
      case "sftp_upload": {
        const dir = a.dir as string;
        const f = dropTokens.get(a.token as number);
        if (!f) throw { code: "not_found", message: "not found" };
        progressThen(a.onEvent as ChannelLike, f.size, { kind: "done", destination: `${dir}/${f.name}` });
        setTimeout(() => remoteFiles[dir]?.push({ name: f.name, kind: "file", size: f.size }), 1500);
        return nextTransfer++;
      }
      case "tunnel_start": {
        const spec = a.spec as { kind: string; localPort: number; allowRemoteClients: boolean; remoteHost?: string; remotePort?: number };
        const id = nextTunnel++;
        tunnels.push({ ...spec, id, terminalId: a.terminalId as number, localAddress: `${spec.allowRemoteClients ? "0.0.0.0" : "127.0.0.1"}:${spec.localPort || 50000 + id}`, exposed: spec.allowRemoteClients, activeConnections: 0, totalConnections: 0 });
        return tunnels.at(-1);
      }
      case "tunnel_list":
        return tunnels.filter((t) => t.terminalId === a.terminalId);
      case "tunnel_stop": {
        const i = tunnels.findIndex((t) => t.id === a.id);
        if (i >= 0) tunnels.splice(i, 1);
        return i >= 0;
      }
      case "transfer_cancel":
        return true;
      case "known_host_forget":
        forgotten.add(`${a.host}:${a.port}`);
        return { removed: 1, remainingInUserFile: 0 };
      case "terminal_write":
        send(a.id as number, String(a.data).replace(/\r/g, "\r\n$ "));
        return null;
      case "terminal_resize":
      case "terminal_ack":
      case "terminal_close":
        return null;
      default:
        throw { code: "internal", message: `mock: unhandled ${cmd}` };
    }
  },
  { shouldMockEvents: true },
);

const dropTokens = new Map<number, { name: string; size: number }>();
let nextToken = 1;
// Simulates dropping a file onto the window (real drag-drop is native).
(window as unknown as { __mockDrop: (name?: string, size?: number) => void }).__mockDrop = (
  name = "notes.txt",
  size = 4096,
) => {
  const token = nextToken++;
  dropTokens.set(token, { name, size });
  void emit("files-dropped", [{ token, name, size }]);
};

const root = document.getElementById("root");
if (!root) throw new Error("missing #root");
createRoot(root).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
