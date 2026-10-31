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
        return { version: "0.0.0-mock", secretsAvailable: true };
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

const root = document.getElementById("root");
if (!root) throw new Error("missing #root");
createRoot(root).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
