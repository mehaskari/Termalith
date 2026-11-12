// Development only: renders the real UI against a simulated backend so the
// layout and dialog flows can be checked in a normal browser. Loaded from
// mock.html, which the production build does not include.
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { emit } from "@tauri-apps/api/event";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { App } from "../App";
import { ErrorBoundary } from "../ui/ErrorBoundary";
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
    { name: "flaky-export.csv", kind: "file", size: 9_000 },
    { name: "backup-2026-10-30.tar.gz", kind: "file", size: 48_234_112 },
  ],
  "/home/deploy/logs": [{ name: "app.log", kind: "file", size: 1_204_331 }],
  "/home/deploy/releases": [],
};
let nextTransfer = 1;
// `?locked` starts with saved passwords locked (master password "master-pass").
let mockMaster = new URLSearchParams(location.search).has("locked") ? { protected: true, locked: true } : { protected: false, locked: false };
let mockMasterPassword = "master-pass";
let mockRepaint: ((w: number, h: number) => void) | undefined;
const mockMacros: { id: string; name: string; steps: { delayMs: number; data: string }[] }[] = [];
const mockSnippets: Array<{ id: string; name: string; command: string; folder: string | null }> = [
  { id: "snp-1", name: "Interface status", command: "show interfaces {{iface}} status", folder: "Cisco" },
  { id: "snp-2", name: "Disk usage", command: "df -h\ndu -sh {{path}}", folder: null },
];
const mockSettings: Record<string, unknown> = { theme: "system", fontSize: 13, fontFamily: "", cursorStyle: "block", cursorBlink: true, scrollback: 5000, logSessions: false, defaultShell: null, copyOnSelect: false, rightClickPaste: false, autoReconnect: true, terminalScheme: "app", proxy: null, updateCheck: true };
const logging = new Set<number>();
let nextTunnel = 1;
const tunnels: Array<Record<string, unknown> & { id: number; terminalId: number }> = [];
const mockServers: Array<{ id: number; port: number; [k: string]: unknown }> = [];
let mockShareFolder = { path: "/Users/me/Downloads/Termalith Share", isDefault: true };
/** Running mock transfers that can be paused: id → pause. */
const pausers = new Map<number, () => void>();
/** Paused mock downloads: id → what a resume needs. */
const pausedDownloads = new Map<number, { name: string; total: number; sent: number }>();
function progressThen(ch: ChannelLike, total: number, done: unknown, pausable?: { id: number; name: string; from?: number }) {
  let index = 0;
  let sent = pausable?.from ?? 0;
  let timer: ReturnType<typeof setTimeout>;
  const send = (message: unknown) => window.__TAURI_INTERNALS__.runCallback(ch.id, { index: index++, message });
  const step = () => {
    sent = Math.min(total, sent + total / 8);
    send({ kind: "progress", transferred: sent, total });
    if (sent < total) timer = setTimeout(step, 400);
    else {
      if (pausable) pausers.delete(pausable.id);
      send(done);
    }
  };
  timer = setTimeout(step, 100);
  if (pausable)
    pausers.set(pausable.id, () => {
      clearTimeout(timer);
      pausers.delete(pausable.id);
      pausedDownloads.set(pausable.id, { name: pausable.name, total, sent });
      send({ kind: "paused" });
    });
}
const retryNames = new Map<number, string>();
const mockKeys = [
  { name: "id_ed25519", algorithm: "ssh-ed25519", fingerprint: "SHA256:Zx0b7m1kQe3lF8yT2vR9cN4pW6sJ5hA1uD3oG7iK0aE", comment: "me@laptop", encrypted: true, hasPrivate: true },
  { name: "id_rsa_old", algorithm: "rsa-sha2-512", fingerprint: "SHA256:Q2w3E4r5T6y7U8i9O0pA1s2D3f4G5h6J7k8L9z0X1c2", comment: "", encrypted: false, hasPrivate: true },
];
const installedKeys = new Set<string>();
const exitChannels = new Map<number, ChannelLike>();
// Dev helper: end SSH terminal `id` as if the connection dropped (no exit status).
(window as unknown as Record<string, unknown>).__dropConnection = (id: number) => {
  const ch = exitChannels.get(id);
  if (ch) window.__TAURI_INTERNALS__.runCallback(ch.id, { index: 0, message: { exit: null } });
};
(window as unknown as Record<string, unknown>).__exitShell = (id: number) => {
  const ch = exitChannels.get(id);
  if (ch) window.__TAURI_INTERNALS__.runCallback(ch.id, { index: 0, message: { exit: { code: 0, signal: null } } });
};
const authWaiters = new Map<number, (answers: string[] | null) => void>();
const remoteText: Record<string, string> = {};
const remoteVersions: Record<string, number> = {};
// Dev helper: simulate someone else editing a file on the server.
(window as unknown as Record<string, unknown>).__touchRemote = (key: string) => {
  remoteVersions[key] = (remoteVersions[key] ?? 1) + 10;
};
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

seed({ name: "web-01", host: "web1.example.com", port: 22, username: "deploy", auth: { kind: "password" }, folder: "Production", tags: ["prod", "web"] });
seed({ name: "db-primary", host: "10.0.0.9", port: 2222, username: "postgres", auth: { kind: "keyFile", path: "/Users/me/.ssh/id_ed25519" }, folder: "Production", tags: ["prod", "web"] });
seed({ name: "lab router", host: "192.168.1.1", port: 22, username: "admin", auth: { kind: "password" }, folder: null }, true);

seed({ name: "win-jump", host: "10.0.5.20", port: 3389, username: "", auth: { kind: "password" }, folder: "Production", jumpVia: "ses-000000000000000000000001", protocol: "rdp", domain: "CORP" });
seed({ name: "core switch", host: "10.0.0.254", port: 23, username: "", auth: { kind: "password" }, folder: "Network", protocol: "telnet" });
seed({ name: "edge-fw", host: "10.0.0.1", port: 22, username: "admin", auth: { kind: "password" }, folder: "Network / Firewalls", icon: { name: "firewall", color: "#e03131" } });
seed({ name: "replica-2", host: "10.0.0.12", port: 22, username: "postgres", auth: { kind: "agent" }, folder: "Production / DB" });
seed({ name: "backups", host: "10.0.1.30", port: 22, username: "admin", auth: { kind: "password" }, folder: null, filesOnly: true });
seed({ name: "file drop", host: "files.example.com", port: 21, username: "admin", auth: { kind: "password" }, folder: null, protocol: "ftps" });
seed({ name: "public mirror", host: "ftp.example.com", port: 21, username: "", auth: { kind: "password" }, folder: null, protocol: "ftp" });
seed({ name: "lab vnc", host: "192.168.1.40", port: 5900, username: "", auth: { kind: "password" }, folder: null, protocol: "vnc" });

const mockFolders = new Set<string>(["Lab/Switches"]);
const mockFolderIcons = new Map<string, { name: string; color?: string | null }>([["Network", { name: "router", color: "#2f9e44" }]]);

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
        // `?portable` runs as a portable copy.
        return {
          version: "0.5.0-mock",
          platform: "macos aarch64",
          secretsAvailable: true,
          agentAvailable: true,
          portable: new URLSearchParams(location.search).has("portable"),
        };
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
        return String(a.json).includes("[Bookmarks")
          ? { created: ["web01", "db primary"], skipped: [], unresolvedJumps: [], notImported: ["telnet box: this MobaXterm session type is not supported"] }
          : { created: [], skipped: [], unresolvedJumps: [], notImported: [] };
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
      case "transfer_history":
        return [
          { id: 3, direction: "upload", name: "nginx.conf", remotePath: "/home/deploy/nginx.conf", localPath: null, size: 2400, outcome: "failed", message: "permission denied", connection: "web-01", finishedAt: Math.floor(Date.now() / 1000) - 60 },
          { id: 2, direction: "download", name: "app.log", remotePath: "/home/deploy/logs/app.log", localPath: "/Users/me/Downloads/app.log", size: 1204331, outcome: "done", message: null, connection: "web-01", finishedAt: Math.floor(Date.now() / 1000) - 600 },
          { id: 1, direction: "download", name: "backup.tar.gz", remotePath: "/home/deploy/backup.tar.gz", localPath: null, size: 48234112, outcome: "cancelled", message: null, connection: "db-primary", finishedAt: Math.floor(Date.now() / 1000) - 7200 },
        ];
      case "transfer_history_clear":
        return null;
      case "crash_reports":
        return new URLSearchParams(location.search).has("crash")
          ? [{ id: "crash-1-0-backend.txt", kind: "backend", time: Math.floor(Date.now() / 1000) - 300, summary: "panic in thread 'tokio-runtime-worker' at crates/app-core/src/terminals.rs:120: index out of bounds", text: "Termalith 0.5.0-mock crash report (backend)\nOS: macos aarch64\nTime: 0\n\npanic in thread 'tokio-runtime-worker' at crates/app-core/src/terminals.rs:120: index out of bounds\n\nBacktrace:\n   0: ...\n" }]
          : [];
      case "crash_dismiss":
      case "frontend_error":
        return null;
      case "open_web_link":
        (window as unknown as Record<string, unknown>).__openedLinks = [
          ...(((window as unknown as Record<string, unknown>).__openedLinks as string[]) ?? []),
          String(a.url),
        ];
        return null;
      case "terminal_save_output":
        return `/Users/me/Documents/Termalith Logs/2026-11-01_10-00-00_${String(a.label)}-output.txt`;
      case "net_scan_start": {
        const ch = a.onResult as ChannelLike;
        const done = a.onDone as ChannelLike;
        const ports = String(a.ports).split(",").map((p) => Number(p.trim())).filter((p) => p > 0);
        let i = 0;
        ports.forEach((port, k) =>
          setTimeout(() => window.__TAURI_INTERNALS__.runCallback(ch.id, { index: i++, message: { port, open: port === 22 || port === 443 } }), 30 * k),
        );
        setTimeout(() => window.__TAURI_INTERNALS__.runCallback(done.id, { index: 0, message: { address: "10.0.0.5", error: null, cancelled: false } }), 30 * ports.length + 50);
        return 7;
      }
      case "net_wake":
        return null;
      case "terminal_log_start":
        logging.add(Number(a.id));
        return { path: `/Users/me/Documents/Termalith Logs/2026-11-01_10-00-00_${String(a.label)}.log`, bytes: 0, dropped: 0 };
      case "terminal_log_stop":
        logging.delete(Number(a.id));
        return null;
      case "keys_list":
        return mockKeys;
      case "key_generate": {
        if (mockKeys.some((k) => k.name === a.name)) throw { code: "destination_exists", message: `destination already exists: ~/.ssh/${a.name}` };
        const k = { name: a.name as string, algorithm: "ssh-ed25519", fingerprint: "SHA256:newKeyNewKeyNewKeyNewKeyNewKeyNewKey000", comment: a.comment as string, encrypted: !!a.passphrase, hasPrivate: true };
        mockKeys.push(k);
        return k;
      }
      case "key_public":
        return `ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIMockMockMock ${a.name}`;
      case "key_install":
        return { added: !installedKeys.has(a.name as string) && !!installedKeys.add(a.name as string), path: "/home/deploy/.ssh/authorized_keys" };
      case "ftp_files_open":
      case "ssh_files_open": {
        const req = a.request as { sessionId?: string; password?: string };
        const s = req.sessionId ? sessions.get(req.sessionId) : undefined;
        if (!s) throw { code: "not_found", message: "not found" };
        if (s.auth.kind === "password" && !s.hasSavedSecret && s.username && req.password !== "secret")
          throw { code: req.password ? "auth_failed" : "password_required", message: "a password is required" };
        return { id: nextTerm++, label: s.name };
      }
      case "telnet_open": {
        const id = nextTerm++;
        outputs.set(id, { ch: a.onOutput as ChannelLike, index: 0 });
        setTimeout(() => send(id, "\r\nUser Access Verification\r\n\r\nUsername: "), 50);
        return id;
      }
      case "terminal_shells":
        return [
          { id: "/bin/zsh", name: "zsh" },
          { id: "/bin/bash", name: "bash" },
          { id: "/opt/homebrew/bin/fish", name: "fish" },
        ];
      case "desktop_clients":
        // `?noclient` simulates a system without RDP/VNC client programs.
        return new URLSearchParams(location.search).has("noclient")
          ? { rdp: null, vnc: null }
          : { rdp: { name: "Microsoft Remote Desktop", asksPasswordFirst: false }, vnc: { name: "Screen Sharing", asksPasswordFirst: false } };
      case "desktop_open": {
        const s = sessions.get(String(a.sessionId));
        if (!s) throw { code: "not_found", message: "not found" };
        const jump = s.jumpVia ? sessions.get(s.jumpVia) : undefined;
        // RDP certificate check as with FreeRDP on Linux; `?rdpchanged`
        // pretends the server renewed its certificate.
        const certKey = `rdp:${s.host}:${s.port}`;
        const changed = new URLSearchParams(location.search).has("rdpchanged");
        if (s.protocol === "rdp" && (!trustedHosts.has(certKey) || changed)) {
          const requestId = nextPrompt++;
          const answer = await new Promise<HostKeyAnswer>((resolve) => {
            hostKeyWaiters.set(requestId, resolve);
            void emit("host-key-prompt", {
              requestId,
              host: s.host,
              port: s.port,
              algorithm: "RDP TLS certificate (SHA-256)",
              fingerprint: "3f:a1:9c:5e:0b:77:d2:41:8e:6a:c3:19:f0:5b:2e:94:71:0d:ac:38:e6:52:9b:1f:c4:07:8a:63:d9:2e:b5:10",
              previousFingerprint: changed
                ? "a2:6c:11:f8:93:4e:0d:b7:5a:c9:28:e1:7f:30:9d:46:bb:02:e5:1c:68:fa:73:9e:04:d1:2b:85:c6:3a:57:e9"
                : undefined,
            });
          });
          if (answer === "reject")
            throw { code: "desktop", message: "could not check the server certificate: the server certificate was not trusted" };
          if (answer === "acceptAndSave") trustedHosts.add(certKey);
        }
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
      case "embedded_open": {
        const login = a.login as { sessionId: string; password: string | null; savePassword: boolean };
        const session = sessions.get(login.sessionId);
        if (!login.password && !session?.hasSavedSecret) throw { code: "password_required", message: "a password is required" };
        if (login.password && login.password !== "secret") throw { code: "auth_failed", message: "rejected" };
        if (login.savePassword && session) session.hasSavedSecret = true;
        // A gradient desktop drawn in strips, like an RDP server would.
        const ev = a.onEvent as ChannelLike;
        const fr = a.onFrame as ChannelLike;
        const [w, h] = [Number(a.width) || 1024, Number(a.height) || 768];
        let evIndex = 0;
        let frIndex = 0;
        const sendEv = (m: unknown) => window.__TAURI_INTERNALS__.runCallback(ev.id, { index: evIndex++, message: m });
        const sendFr = (m: unknown) => window.__TAURI_INTERNALS__.runCallback(fr.id, { index: frIndex++, message: m });
        // A resize sends only frames of the new size, like a server whose size
        // event never reaches the viewer.
        const paint = (w: number, h: number) => {
          for (let y = 0; y < h; y += 64) {
            const rows = Math.min(64, h - y);
            const buf = new ArrayBuffer(12 + w * rows * 4);
            const v = new DataView(buf);
            [0, y, w, rows, w, h].forEach((n, i) => v.setUint16(i * 2, n, true));
            const px = new Uint8ClampedArray(buf, 12);
            for (let i = 0; i < w * rows; i++) {
              px[i * 4] = 20 + ((i % w) * 60) / w;
              px[i * 4 + 1] = 60 + ((y + i / w) * 80) / h;
              px[i * 4 + 2] = 120;
              px[i * 4 + 3] = 255;
            }
            sendFr(buf);
          }
        };
        mockRepaint = paint;
        setTimeout(() => {
          sendEv({ kind: "connected", width: w, height: h });
          paint(w, h);
          // A pointer shape, like a server's resize arrow: a 16×16 triangle.
          const rgba: number[] = [];
          for (let y = 0; y < 16; y++)
            for (let x = 0; x < 16; x++) rgba.push(255, 255, 255, x <= y ? 255 : 0);
          sendEv({ kind: "cursor", hidden: false, image: { width: 16, height: 16, hotX: 0, hotY: 0, rgba } });
        }, 300);
        return { id: 1 };
      }
      case "embedded_input":
      case "embedded_ack":
      case "embedded_resize":
        setTimeout(() => mockRepaint?.(Number(a.width), Number(a.height)), 100);
        return null;
      case "embedded_clipboard":
      case "embedded_close":
        return null;
      case "server_stats":
        return {
          cpuPercent: 12 + Math.random() * 30,
          load1: 0.42,
          memTotal: 8 * 1024 ** 3,
          memUsed: 3.1 * 1024 ** 3,
          diskTotal: 100 * 1024 ** 3,
          diskUsed: 42 * 1024 ** 3,
          rxPerSec: 120000,
          txPerSec: 34000,
        };
      case "server_stats_stop":
        return null;
      case "update_check":
        return {
          current: "0.5.0",
          latest: "0.6.0",
          newer: true,
          url: "https://github.com/mehaskari/Termalith/releases/tag/v0.6.0",
          prerelease: true,
        };
      case "proxy_password_status":
        return mockProxyPassword;
      case "proxy_password_set":
        mockProxyPassword = a.password !== null;
        return null;
      case "open_local_network_settings":
      case "app_restart":
        return null;
      case "desktop_list":
        return [...desktops.values()];
      case "desktop_close":
        desktops.delete(Number(a.id));
        return null;
      case "sessions_list":
        return [...sessions.values()];
      case "folders_list":
        return [...mockFolders];
      case "folder_icons_list":
        return [...mockFolderIcons.entries()];
      case "folder_icon_set":
        if (a.icon) mockFolderIcons.set(String(a.path), a.icon as { name: string; color?: string | null });
        else mockFolderIcons.delete(String(a.path));
        return null;
      case "folder_create": {
        const path = String(a.path).split("/").map((x) => x.trim()).filter(Boolean).join("/");
        if (!path) throw { code: "invalid", message: "invalid folder: must not be empty" };
        mockFolders.add(path);
        return path;
      }
      case "folder_rename": {
        const norm = (f: string) => f.split("/").map((x) => x.trim()).filter(Boolean).join("/");
        const from = norm(String(a.from));
        const to = norm(String(a.to));
        if (to.toLowerCase().startsWith(`${from.toLowerCase()}/`)) throw { code: "invalid", message: "invalid folder: cannot move a folder into itself" };
        const moved = (p: string) => {
          const n = norm(p);
          if (n.toLowerCase() === from.toLowerCase()) return to;
          if (n.toLowerCase().startsWith(`${from.toLowerCase()}/`)) return to + n.slice(from.length);
          return null;
        };
        for (const s of sessions.values()) {
          const m = s.folder ? moved(s.folder) : null;
          if (m) s.folder = m;
        }
        for (const f of [...mockFolders]) {
          const m = moved(f);
          if (m) {
            mockFolders.delete(f);
            mockFolders.add(m);
          }
        }
        mockFolders.add(to);
        return to;
      }
      case "folder_delete": {
        const path = String(a.path).toLowerCase();
        const inside = (f: string) => f.toLowerCase() === path || f.toLowerCase().startsWith(`${path}/`);
        if ([...sessions.values()].some((s) => s.folder && inside(s.folder.split("/").map((x) => x.trim()).join("/"))))
          throw { code: "invalid", message: "invalid folder: move or delete the sessions in it first" };
        for (const f of [...mockFolders]) if (inside(f)) mockFolders.delete(f);
        return null;
      }
      case "session_move": {
        const s = sessions.get(a.id as string);
        if (s) s.folder = (a.folder as string | null) ?? null;
        return null;
      }
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
      case "master_status":
        return mockMaster;
      case "master_enable":
        mockMaster = { protected: true, locked: false };
        mockMasterPassword = a.password as string;
        return null;
      case "master_unlock":
        if (a.password !== mockMasterPassword) throw { code: "wrong_master_password", message: "wrong master password" };
        mockMaster = { protected: true, locked: false };
        return null;
      case "master_lock":
        if (mockMaster.protected) mockMaster = { protected: true, locked: true };
        return null;
      case "master_change":
        if (a.old !== mockMasterPassword) throw { code: "wrong_master_password", message: "wrong master password" };
        mockMasterPassword = a.new as string;
        return null;
      case "master_disable":
        if (a.password !== mockMasterPassword) throw { code: "wrong_master_password", message: "wrong master password" };
        mockMaster = { protected: false, locked: false };
        return null;
      case "master_reset":
        mockMaster = { protected: false, locked: false };
        for (const s of sessions.values()) s.hasSavedSecret = false;
        return null;
      case "macros_list":
        return mockMacros;
      case "macro_create": {
        const m = { id: `mac-${mockMacros.length + 1}`, ...(a.input as object) } as (typeof mockMacros)[number];
        mockMacros.push(m);
        return m;
      }
      case "macro_delete":
        mockMacros.splice(mockMacros.findIndex((m) => m.id === a.id), 1);
        return null;
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
      case "mosh_open": {
        const id = nextTerm++;
        setTimeout(() => send(id, "\x1b[2m(mock mosh)\x1b[0m\r\nops@edge:~$ "), 50);
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
        // `?nolan` pretends macOS blocks the local network.
        if (new URLSearchParams(location.search).has("nolan"))
          throw {
            code: "local_network_blocked",
            message:
              "could not connect: No route to host (os error 65) — macOS is probably blocking Termalith from your local network: turn Termalith on in System Settings → Privacy & Security → Local Network, then quit and reopen Termalith",
          };
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
        // db-primary simulates a server asking for a one-time code.
        if (target.host === "10.0.0.9") {
          const requestId = nextPrompt++;
          const answers = await new Promise<string[] | null>((resolve) => {
            authWaiters.set(requestId, resolve);
            void emit("auth-prompt", {
              requestId,
              host: target.host,
              port: target.port,
              user: target.username,
              name: "Two-factor authentication",
              instructions: "Open your authenticator app.\nCodes change every 30 seconds.",
              fields: [{ text: "Verification code:", echo: true }],
            });
          });
          if (!answers) throw { code: "cancelled", message: "cancelled" };
          if (answers[0] !== "123456") throw { code: "auth_failed", message: "authentication failed" };
        }
        const id = nextTerm++;
        outputs.set(id, { ch: a.onOutput as ChannelLike, index: 0 });
        exitChannels.set(id, a.onExit as ChannelLike);
        setTimeout(() => send(id, `Connected to ${target.host} (mock)\r\n${target.username}@${target.host}:~$ `), 50);
        return { id, label: target.name || `${target.username}@${target.host}` };
      }
      case "auth_prompt_respond": {
        const w = authWaiters.get(a.requestId as number);
        authWaiters.delete(a.requestId as number);
        w?.((a.answers as string[] | null) ?? null);
        return w !== undefined;
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
        const size = (a.size as number | null) ?? (a.folder ? 250_000 : 1000);
        const fails = String(a.name).includes("flaky");
        const id = nextTransfer++;
        if (fails) retryNames.set(id, String(a.name));
        progressThen(
          a.onEvent as ChannelLike,
          size,
          fails
            ? { kind: "failed", message: "SFTP error: connection lost", retryable: true }
            : { kind: "done", destination: `/Users/me/Downloads/${a.name}` },
          fails ? undefined : { id, name: String(a.name) },
        );
        return { id, downloadsDir: "/Users/me/Downloads" };
      }
      case "sftp_mkdir": {
        const list = (remoteFiles[a.dir as string] ??= []);
        if (list.some((e) => e.name === a.name)) throw { code: "destination_exists", message: `destination already exists: ${a.dir}/${a.name}` };
        list.push({ name: a.name as string, kind: "dir", size: null });
        remoteFiles[`${a.dir}/${a.name}`] = [];
        return null;
      }
      case "sftp_rename": {
        const list = remoteFiles[a.dir as string] ?? [];
        if (list.some((e) => e.name === a.to)) throw { code: "destination_exists", message: `destination already exists: ${a.dir}/${a.to}` };
        const e = list.find((x) => x.name === a.from);
        if (e) e.name = a.to as string;
        return null;
      }
      case "sftp_delete": {
        const dir = a.dir as string;
        const key = `${dir}/${a.name}`;
        if ((remoteFiles[key]?.length ?? 0) > 0) throw { code: "sftp_error", message: "SFTP error: could not delete the folder (is it empty?): Failure" };
        remoteFiles[dir] = (remoteFiles[dir] ?? []).filter((e) => e.name !== a.name);
        return null;
      }
      case "sftp_read_text": {
        const key = `${a.dir}/${a.name}`;
        if (!(key in remoteText)) remoteText[key] = `# ${a.name}\nlisten 8080\nworkers 4\n`;
        const content = remoteText[key] ?? "";
        return { path: key, content, version: { size: content.length, modified: remoteVersions[key] ?? 1 } };
      }
      case "sftp_save_text": {
        const key = `${a.dir}/${a.name}`;
        const v = a.version as { modified: number | null };
        if ((remoteVersions[key] ?? 1) !== v.modified) throw { code: "remote_modified", message: `${key} was changed on the server since it was opened` };
        remoteText[key] = String(a.content);
        remoteVersions[key] = (remoteVersions[key] ?? 1) + 1;
        return { size: remoteText[key].length, modified: remoteVersions[key] };
      }
      case "transfer_retry": {
        const p = pausedDownloads.get(Number(a.id));
        if (p) {
          pausedDownloads.delete(Number(a.id));
          const id = nextTransfer++;
          progressThen(a.onEvent as ChannelLike, p.total, { kind: "done", destination: `/Users/me/Downloads/${p.name}` }, { id, name: p.name, from: p.sent });
          return id;
        }
        const name = retryNames.get(Number(a.id));
        if (!name) throw { code: "not_found", message: "not found" };
        retryNames.delete(Number(a.id));
        progressThen(a.onEvent as ChannelLike, 1000, { kind: "done", destination: `/Users/me/Downloads/${name}` });
        return nextTransfer++;
      }
      case "sftp_upload": {
        const dir = a.dir as string;
        const f = dropTokens.get(a.token as number);
        if (!f) throw { code: "not_found", message: "not found" };
        progressThen(a.onEvent as ChannelLike, f.size, { kind: "done", destination: `${dir}/${f.name}` });
        setTimeout(
          () => remoteFiles[dir]?.push({ name: f.name, kind: f.folder ? "dir" : "file", size: f.folder ? null : f.size }),
          1500,
        );
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
      case "server_folder":
        return { ...mockShareFolder };
      case "server_pick_folder":
        mockShareFolder = { path: "/Users/me/Firmware", isDefault: false };
        return { ...mockShareFolder };
      case "server_use_dropped":
        mockShareFolder = { path: "/Users/me/Desktop/Dropped folder", isDefault: false };
        return { ...mockShareFolder };
      case "server_reset_folder":
        mockShareFolder = { path: "/Users/me/Downloads/Termalith Share", isDefault: true };
        return { ...mockShareFolder };
      case "server_open_folder":
        return null;
      case "server_start": {
        const r = a.request as { kind: "tftp" | "http" | "ftp"; lan: boolean; port: number | null; allowUploads: boolean; requireLogin: boolean; user: string | null; password: string | null };
        const port = r.port ?? { tftp: 6969, http: 8080, ftp: 2121 }[r.kind];
        if (mockServers.some((x) => x.port === port)) throw { code: "invalid", message: `invalid server: cannot listen on 0.0.0.0:${port}: Address already in use` };
        const id = mockServers.length + 1;
        const scheme = r.kind;
        const urls = (r.lan ? ["192.168.1.23", "127.0.0.1"] : ["127.0.0.1"]).map((h) => `${scheme}://${h}:${port}/`);
        const now = Math.floor(Date.now() / 1000);
        const server = {
          id, kind: r.kind, folder: mockShareFolder.path, port, lan: r.lan, allowUploads: r.allowUploads, urls,
          credentials: r.kind === "ftp" || (r.kind === "http" && r.requireLogin) ? { user: r.user || "termalith", password: r.password || "k7Qm2xPa9TbN" } : null,
          activity: [
            { seq: 1, at: now, peer: "192.168.1.40", action: "download", path: "/fw/switch-9.3.bin", bytes: 48_234_496, error: null },
            { seq: 2, at: now, peer: "192.168.1.40", action: "upload", path: "/backup.cfg", bytes: 0, error: "uploads are turned off" },
          ],
        };
        mockServers.push(server);
        return server;
      }
      case "server_list":
        return mockServers.map((x) => ({ ...x }));
      case "server_stop": {
        const i = mockServers.findIndex((x) => x.id === a.id);
        if (i >= 0) mockServers.splice(i, 1);
        return i >= 0;
      }
      case "transfer_cancel":
        pausedDownloads.delete(Number(a.id));
        return true;
      case "transfer_pause": {
        const pause = pausers.get(Number(a.id));
        pause?.();
        return Boolean(pause);
      }
      case "known_host_forget":
        forgotten.add(`${a.host}:${a.port}`);
        return { removed: 1, remainingInUserFile: 0 };
      case "terminal_write":
        if (String(a.data).includes("__termalith_cwd")) send(a.id as number, "\x1b]7;file://web1/home/deploy/logs\x1b\\");
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

let mockProxyPassword = false;
const dropTokens = new Map<number, { name: string; size: number; folder: boolean }>();
let nextToken = 1;
// Simulates dragging over and dropping onto the window (real drag-drop is
// native). `__mockHover(true)` shows the drop target.
const mockWindow = window as unknown as {
  __mockDrop: (name?: string, size?: number, folder?: boolean) => void;
  __mockHover: (on: boolean) => void;
};
mockWindow.__mockDrop = (name = "notes.txt", size = 4096, folder = false) => {
  const token = nextToken++;
  dropTokens.set(token, { name, size, folder });
  void emit("files-hover", false);
  void emit("files-dropped", [{ token, name, size, folder }]);
};
mockWindow.__mockHover = (on) => void emit("files-hover", on);

// `?uicrash` shows the recovery screen.
function Boom(): never {
  throw new TypeError("mock render failure");
}

const root = document.getElementById("root");
if (!root) throw new Error("missing #root");
createRoot(root).render(
  <StrictMode>
    <ErrorBoundary>{new URLSearchParams(location.search).has("uicrash") ? <Boom /> : <App />}</ErrorBoundary>
  </StrictMode>,
);
