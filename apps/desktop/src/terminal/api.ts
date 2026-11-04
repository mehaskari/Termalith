// Typed wrappers around the IPC commands. The backend validates all
// arguments; nothing here is a security boundary.
import { Channel, type InvokeArgs, invoke as tauriInvoke } from "@tauri-apps/api/core";

export type TerminalId = number;

export interface ExitPayload {
  exit: { code: number; signal: string | null } | null;
}

export interface IpcError {
  code: string;
  message: string;
  detail?: unknown;
}

export function isIpcError(e: unknown): e is IpcError {
  return (
    typeof e === "object" &&
    e !== null &&
    typeof (e as IpcError).code === "string" &&
    typeof (e as IpcError).message === "string"
  );
}

type BlockedListener = (e: IpcError) => void;
const localNetworkListeners = new Set<BlockedListener>();

/**
 * Called whenever a command fails because macOS (probably) blocks the local
 * network for Termalith. Returns an unsubscribe function.
 */
export function onLocalNetworkBlocked(listener: BlockedListener): () => void {
  localNetworkListeners.add(listener);
  return () => localNetworkListeners.delete(listener);
}

/** Every command goes through here so blocked connections are noticed in one place. */
async function invoke<T>(cmd: string, args?: InvokeArgs): Promise<T> {
  try {
    return await tauriInvoke<T>(cmd, args);
  } catch (e) {
    if (isIpcError(e) && e.code === "local_network_blocked") for (const l of localNetworkListeners) l(e);
    throw e;
  }
}

export const openLocalNetworkSettings = () => invoke<void>("open_local_network_settings");
export const restartApp = () => invoke<void>("app_restart");

/** Opens a terminal of the given size and wires its output/exit streams. */
export type Opener = (
  cols: number,
  rows: number,
  onOutput: (data: Uint8Array) => void,
  onExit: (payload: ExitPayload) => void,
) => Promise<TerminalId>;

function channels(onOutput: (data: Uint8Array) => void, onExit: (p: ExitPayload) => void) {
  const output = new Channel<ArrayBuffer>();
  output.onmessage = (buf) => onOutput(new Uint8Array(buf));
  const exit = new Channel<ExitPayload>();
  exit.onmessage = onExit;
  return { onOutput: output, onExit: exit };
}

/** A local shell; `shell` is an id from `listShells` (default: system shell). */
export const localTerminal =
  (shell?: string | null): Opener =>
  (cols, rows, onOutput, onExit) =>
    invoke<TerminalId>("terminal_open", { cols, rows, shell: shell ?? null, ...channels(onOutput, onExit) });

export const openLocalTerminal: Opener = localTerminal();

export interface ShellChoice {
  id: string;
  name: string;
}

/** Installed shells, the system default first. */
export function listShells(): Promise<ShellChoice[]> {
  return invoke("terminal_shells");
}

/** Another terminal next to `source`: a local shell, or a shell on the same SSH connection. */
export const splitTerminal =
  (source: TerminalId): Opener =>
  (cols, rows, onOutput, onExit) =>
    invoke<TerminalId>("terminal_split", { source, cols, rows, ...channels(onOutput, onExit) });

export function writeTerminal(id: TerminalId, data: string): Promise<void> {
  return invoke("terminal_write", { id, data });
}

export function resizeTerminal(id: TerminalId, cols: number, rows: number): Promise<void> {
  return invoke("terminal_resize", { id, cols, rows });
}

export function ackTerminal(id: TerminalId, bytes: number): Promise<void> {
  return invoke("terminal_ack", { id, bytes });
}

export function closeTerminal(id: TerminalId): Promise<void> {
  return invoke("terminal_close", { id });
}

// ---------------------------------------------------------------- sessions

export type AuthMethod = { kind: "password" } | { kind: "keyFile"; path: string } | { kind: "agent" };

export type SessionProtocol = "ssh" | "rdp" | "vnc" | "telnet";

export interface SessionInput {
  name: string;
  host: string;
  port: number;
  username: string;
  auth: AuthMethod;
  folder: string | null;
  /** Saved session to tunnel through (ProxyJump). */
  jumpVia?: string | null;
  /** Defaults to "ssh" when absent. */
  protocol?: SessionProtocol;
  /** Windows domain (RDP only). */
  domain?: string | null;
  tags?: string[];
  /** Forward the local SSH agent to this host (SSH only). */
  forwardAgent?: boolean;
  /** Forward X11 to the local X server (SSH only). */
  x11Forward?: boolean;
  /** Typed into the shell once after login (SSH only). */
  startupCommand?: string | null;
  /** Opens as a file browser (SFTP) without a terminal. */
  filesOnly?: boolean;
}

export interface SessionRecord extends SessionInput {
  id: string;
  hasSavedSecret: boolean;
  favorite?: boolean;
  updatedAt: number;
}

export interface AppInfo {
  version: string;
  /** OS and architecture, e.g. "macos aarch64". */
  platform?: string;
  secretsAvailable: boolean;
  agentAvailable: boolean;
}

export interface ImportCandidate {
  alias: string;
  input: SessionInput;
  warnings: string[];
  jumpAlias: string | null;
}

export const sshConfigPreview = () => invoke<ImportCandidate[]>("ssh_config_preview");
export const sshConfigImport = (aliases: string[]) =>
  invoke<{ created: string[]; skipped: string[]; unresolvedJumps: string[] }>("ssh_config_import", { aliases });

export const appInfo = () => invoke<AppInfo>("app_info");

/** Whether a proxy password is saved; the password never reaches the UI. */
export const proxyPasswordStatus = () => invoke<boolean>("proxy_password_status");
/** Saves the proxy password in the OS credential store; null removes it. */
export const setProxyPassword = (password: string | null) => invoke<void>("proxy_password_set", { password });

export interface UpdateInfo {
  current: string;
  latest: string | null;
  newer: boolean;
  /** Release page on github.com. */
  url: string | null;
  prerelease: boolean;
}

/** Asks GitHub for the newest published release; never downloads anything. */
export const checkForUpdates = () => invoke<UpdateInfo>("update_check");
export const listSessions = () => invoke<SessionRecord[]>("sessions_list");
export const createSession = (input: SessionInput) =>
  invoke<SessionRecord>("session_create", { input });
export const updateSession = (id: string, input: SessionInput) =>
  invoke<SessionRecord>("session_update", { id, input });
export const deleteSession = (id: string) => invoke<void>("session_delete", { id });
export const forgetSessionSecret = (id: string) => invoke<void>("session_forget_secret", { id });

// ---------------------------------------------------------------- SSH

export interface QuickTarget {
  host: string;
  port: number;
  username: string;
  auth: AuthMethod;
}

export type SshTarget = { sessionId: string } | { quick: QuickTarget };

export interface SshOpenRequest {
  sessionId?: string;
  quick?: QuickTarget;
  /** Sent once for this connection attempt; never stored in the UI. */
  password?: string;
  savePassword?: boolean;
}

export async function openSsh(
  request: SshOpenRequest,
  cols: number,
  rows: number,
  onOutput: (data: Uint8Array) => void,
  onExit: (payload: ExitPayload) => void,
): Promise<{ id: TerminalId; label: string }> {
  return invoke("ssh_open", { request, cols, rows, ...channels(onOutput, onExit) });
}

export interface HostKeyPrompt {
  requestId: number;
  host: string;
  port: number;
  algorithm: string;
  fingerprint: string;
  /** RDP only: the certificate trusted before, when the server's changed. */
  previousFingerprint?: string;
}

/** Algorithm label the backend uses for RDP certificates. */
export const RDP_CERT_ALGORITHM = "RDP TLS certificate (SHA-256)";

export type HostKeyAnswer = "reject" | "acceptOnce" | "acceptAndSave";

export const respondHostKey = (requestId: number, answer: HostKeyAnswer) =>
  invoke<boolean>("host_key_respond", { requestId, answer });

// ---------------------------------------------------------------- files

export interface RemoteEntry {
  name: string;
  kind: "dir" | "file" | "symlink" | "other";
  size: number | null;
  modified: number | null;
  permissions: number | null;
}

export type TransferEvent =
  | { kind: "queued" }
  | { kind: "progress"; transferred: number; total: number | null }
  | { kind: "retrying"; attempt: number; max: number; message: string }
  | { kind: "done"; destination: string }
  | { kind: "failed"; message: string; retryable: boolean }
  | { kind: "cancelled" };

export interface DroppedFile {
  token: number;
  name: string;
  /** Bytes; for a folder, the total of the files in it. */
  size: number;
  folder: boolean;
}

export const sftpHome = (terminalId: TerminalId) => invoke<string>("sftp_home", { terminalId });
export const sftpList = (terminalId: TerminalId, path: string) =>
  invoke<RemoteEntry[]>("sftp_list", { terminalId, path });

function transferChannel(onEvent: (e: TransferEvent) => void) {
  const ch = new Channel<TransferEvent>();
  ch.onmessage = onEvent;
  return ch;
}

export const sftpDownload = (
  terminalId: TerminalId,
  dir: string,
  name: string,
  size: number | null,
  onEvent: (e: TransferEvent) => void,
  connection?: string,
) =>
  invoke<{ id: number; downloadsDir: string }>("sftp_download", {
    terminalId,
    dir,
    name,
    size,
    connection: connection ?? null,
    onEvent: transferChannel(onEvent),
  });

export const sftpUpload = (
  terminalId: TerminalId,
  token: number,
  dir: string,
  onEvent: (e: TransferEvent) => void,
  connection?: string,
) => invoke<number>("sftp_upload", { terminalId, token, dir, connection: connection ?? null, onEvent: transferChannel(onEvent) });

export const cancelTransfer = (id: number) => invoke<boolean>("transfer_cancel", { id });

/** Runs a failed or cancelled transfer again; resolves to the new id. */
export const retryTransfer = (id: number, onEvent: (e: TransferEvent) => void) =>
  invoke<number>("transfer_retry", { id, onEvent: transferChannel(onEvent) });

export const forgetHostKey = (host: string, port: number) =>
  invoke<{ removed: number; remainingInUserFile: number }>("known_host_forget", { host, port });

// ---------------------------------------------------------------- tunnels

export type TunnelKind =
  | { kind: "local"; remoteHost: string; remotePort: number }
  | { kind: "socks" }
  | { kind: "remote"; targetHost: string; targetPort: number };

export type TunnelSpec = TunnelKind & { localPort: number; allowRemoteClients: boolean };

export type TunnelInfo = TunnelKind & {
  id: number;
  terminalId: number;
  localAddress: string;
  exposed: boolean;
  activeConnections: number;
  totalConnections: number;
};

export const startTunnel = (terminalId: TerminalId, spec: TunnelSpec) =>
  invoke<TunnelInfo>("tunnel_start", { terminalId, spec });
export const listTunnels = (terminalId: TerminalId) => invoke<TunnelInfo[]>("tunnel_list", { terminalId });
export const stopTunnel = (id: number) => invoke<boolean>("tunnel_stop", { id });

// ---------------------------------------------------------------- settings

export interface Settings {
  theme: "system" | "dark" | "light";
  fontSize: number;
  fontFamily: string;
  cursorStyle: "block" | "bar" | "underline";
  cursorBlink: boolean;
  scrollback: number;
  /** Start a session log for every new terminal. */
  logSessions: boolean;
  /** Shell id for new local terminals; null = system default. */
  defaultShell: string | null;
  /** Copy terminal selections to the clipboard automatically. */
  copyOnSelect: boolean;
  /** Right-click in a terminal pastes the clipboard. */
  rightClickPaste: boolean;
  /** Reconnect SSH tabs whose connection was lost (not on `exit`). */
  autoReconnect: boolean;
  /** Terminal color scheme id (see TERMINAL_SCHEMES); "app" follows the app theme. */
  terminalScheme: string;
  /** Proxy for outgoing SSH/Telnet connections (first hop only). */
  proxy: { kind: "socks5" | "http"; host: string; port: number; username?: string | null } | null;
  /** Look for a newer release on GitHub at startup. */
  checkUpdates: boolean;
  /** Show RDP sessions in a tab (experimental) instead of the external client. */
  embeddedRdp: boolean;
}

export const DEFAULT_SETTINGS: Settings = {
  theme: "system",
  fontSize: 13,
  fontFamily: "",
  cursorStyle: "block",
  cursorBlink: true,
  scrollback: 5000,
  logSessions: false,
  defaultShell: null,
  copyOnSelect: false,
  rightClickPaste: false,
  autoReconnect: true,
  terminalScheme: "app",
  proxy: null,
  checkUpdates: false,
  embeddedRdp: false,
};

export const getSettings = () => invoke<Settings>("settings_get");
export const saveSettings = (settings: Settings) => invoke<void>("settings_save", { settings });
export const setFavorite = (id: string, favorite: boolean) => invoke<void>("session_set_favorite", { id, favorite });

// ---------------------------------------------------------------- folders

/** Folders stored on their own; others exist only through their sessions. */
export const listFolders = () => invoke<string[]>("folders_list");
/** Returns the normalized path ("A / B" becomes "A/B"). */
export const createFolder = (path: string) => invoke<string>("folder_create", { path });
/** Renames or moves a folder with its subfolders and sessions. */
export const renameFolder = (from: string, to: string) => invoke<string>("folder_rename", { from, to });
/** Refused while sessions are inside. */
export const deleteFolder = (path: string) => invoke<void>("folder_delete", { path });
export const moveSession = (id: string, folder: string | null) => invoke<void>("session_move", { id, folder });
export const exportSessions = () => invoke<{ json: string; savedTo: string | null }>("sessions_export");
/** Accepts a Termalith export or a MobaXterm `.mxtsessions` file. */
export const importSessions = (text: string) =>
  invoke<{ created: string[]; skipped: string[]; unresolvedJumps: string[]; notImported: string[] }>(
    "sessions_import",
    { json: text },
  );

// ---------------------------------------------------------------- serial

export interface SerialConfig {
  path: string;
  baudRate: number;
  dataBits: number;
  parity: "none" | "odd" | "even";
  stopBits: number;
  flowControl: "none" | "software" | "hardware";
}

export interface SerialPortInfo {
  path: string;
  description: string;
}

export const serialPorts = () => invoke<SerialPortInfo[]>("serial_ports");

export const openSerial =
  (config: SerialConfig): Opener =>
  (_cols, _rows, onOutput, onExit) =>
    invoke<TerminalId>("serial_open", { config, ...channels(onOutput, onExit) });

export function serialSummary(c: SerialConfig): string {
  return `${c.baudRate} ${c.dataBits}${c.parity[0]?.toUpperCase() ?? "N"}${c.stopBits}`;
}

// ---------------------------------------------------------------- snippets

export interface SnippetInput {
  name: string;
  command: string;
  folder: string | null;
}

export interface Snippet extends SnippetInput {
  id: string;
}

export const listSnippets = () => invoke<Snippet[]>("snippets_list");
export const createSnippet = (input: SnippetInput) => invoke<Snippet>("snippet_create", { input });
export const updateSnippet = (id: string, input: SnippetInput) => invoke<Snippet>("snippet_update", { id, input });
export const deleteSnippet = (id: string) => invoke<void>("snippet_delete", { id });

// ---------------------------------------------------------------- network tools

export interface TcpCheck {
  open: boolean;
  address: string | null;
  millis: number;
  error: string | null;
}

export const tcpCheck = (host: string, port: number) => invoke<TcpCheck>("net_tcp_check", { host, port });
export const dnsLookup = (name: string) => invoke<string[]>("net_dns_lookup", { name });

export function startProbe(
  kind: "ping" | "traceroute",
  host: string,
  onLine: (line: string) => void,
  onDone: (code: number | null) => void,
): Promise<number> {
  const line = new Channel<string>();
  line.onmessage = onLine;
  const done = new Channel<number | null>();
  done.onmessage = onDone;
  return invoke<number>("net_probe_start", { kind, host, onLine: line, onDone: done });
}

export const cancelProbe = (id: number) => invoke<boolean>("net_probe_cancel", { id });

export interface ScanDone {
  address: string | null;
  error: string | null;
  cancelled: boolean;
}

/** TCP connect scan ("22,80,8000-8010", at most 1024 ports); cancel with cancelProbe. */
export function startScan(
  host: string,
  ports: string,
  onResult: (r: { port: number; open: boolean }) => void,
  onDone: (d: ScanDone) => void,
): Promise<number> {
  const result = new Channel<{ port: number; open: boolean }>();
  result.onmessage = onResult;
  const done = new Channel<ScanDone>();
  done.onmessage = onDone;
  return invoke<number>("net_scan_start", { host, ports, onResult: result, onDone: done });
}

/** Sends a Wake-on-LAN magic packet. */
export const wakeOnLan = (mac: string, broadcast: string | null) => invoke<void>("net_wake", { mac, broadcast });

/** Saves terminal text to a new file in the logs folder; resolves to its path. */
export const saveTerminalOutput = (label: string, text: string) =>
  invoke<string>("terminal_save_output", { label, text });

// ---------------------------------------------------------------- remote desktop

export interface DesktopClient {
  name: string;
  /** The client reads the password before it starts (FreeRDP). */
  asksPasswordFirst: boolean;
}

export interface DesktopClients {
  rdp: DesktopClient | null;
  vnc: DesktopClient | null;
}

export type DesktopState = { kind: "running" } | { kind: "handedOff" } | { kind: "exited"; code: number | null };

export type DesktopInfo = DesktopState & {
  id: number;
  sessionId: string | null;
  label: string;
  protocol: "rdp" | "vnc";
  client: string;
  address: string;
  via: string | null;
};

export function desktopClients(): Promise<DesktopClients> {
  return invoke("desktop_clients");
}

export function openDesktop(sessionId: string, password?: string): Promise<DesktopInfo> {
  return invoke("desktop_open", { sessionId, password: password ?? null });
}

export function listDesktops(): Promise<DesktopInfo[]> {
  return invoke("desktop_list");
}

// Embedded RDP viewer (experimental, ADR-030).

export type EmbeddedEvent =
  | { kind: "connected"; width: number; height: number }
  | { kind: "clipboard"; text: string }
  | { kind: "closed"; reason: string | null };

export type EmbeddedInput =
  | { kind: "move"; x: number; y: number }
  | { kind: "button"; button: number; down: boolean }
  | { kind: "wheel"; vertical: boolean; units: number }
  | { kind: "key"; scancode: number; down: boolean }
  | { kind: "text"; ch: string; down: boolean }
  | { kind: "releaseAll" };

/** A changed region: RGBA rows. */
export interface EmbeddedFrame {
  x: number;
  y: number;
  w: number;
  h: number;
  rgba: Uint8ClampedArray;
}

/** Parses the frame wire format: x, y, w, h (u16 LE), then RGBA. */
export function parseFrame(buf: ArrayBuffer): EmbeddedFrame | null {
  if (buf.byteLength < 8) return null;
  const v = new DataView(buf);
  const [x, y, w, h] = [0, 2, 4, 6].map((o) => v.getUint16(o, true)) as [number, number, number, number];
  if (buf.byteLength !== 8 + w * h * 4 || w === 0 || h === 0) return null;
  return { x, y, w, h, rgba: new Uint8ClampedArray(buf, 8) };
}

export function openEmbedded(
  sessionId: string,
  password: string,
  width: number,
  height: number,
  onFrame: (f: EmbeddedFrame) => void,
  onEvent: (e: EmbeddedEvent) => void,
): Promise<number> {
  const frames = new Channel<ArrayBuffer>();
  frames.onmessage = (buf) => {
    const f = parseFrame(buf);
    if (f) onFrame(f);
  };
  const events = new Channel<EmbeddedEvent>();
  events.onmessage = onEvent;
  return invoke("embedded_open", { sessionId, password, width, height, onFrame: frames, onEvent: events });
}

export const embeddedInput = (id: number, events: EmbeddedInput[]) => invoke<void>("embedded_input", { id, events });
export const embeddedAck = (id: number) => invoke<void>("embedded_ack", { id });
/** Offers text copied here to the remote computer. */
export const embeddedClipboard = (id: number, text: string) => invoke<void>("embedded_clipboard", { id, text });
export const embeddedResize = (id: number, width: number, height: number) =>
  invoke<void>("embedded_resize", { id, width, height });
export const closeEmbedded = (id: number) => invoke<void>("embedded_close", { id });

export function closeDesktop(id: number): Promise<void> {
  return invoke("desktop_close", { id });
}

// ---------------------------------------------------------------- session logs

export interface LogInfo {
  /** Plain-text log file, in the logs folder chosen by the backend. */
  path: string;
  bytes: number;
  dropped: number;
}

export function startLog(id: TerminalId, label: string): Promise<LogInfo> {
  return invoke("terminal_log_start", { id, label });
}

export function stopLog(id: TerminalId): Promise<LogInfo | null> {
  return invoke("terminal_log_stop", { id });
}

// ---------------------------------------------------------------- remote editor

/** Identifies the file content a save is based on (conflict detection). */
export interface EditVersion {
  size: number | null;
  modified: number | null;
}

export interface RemoteText {
  path: string;
  content: string;
  version: EditVersion;
}

export function readRemoteText(terminalId: TerminalId, dir: string, name: string): Promise<RemoteText> {
  return invoke("sftp_read_text", { terminalId, dir, name });
}

export function saveRemoteText(
  terminalId: TerminalId,
  dir: string,
  name: string,
  content: string,
  version: EditVersion,
): Promise<EditVersion> {
  return invoke("sftp_save_text", { terminalId, dir, name, content, version });
}

// ---------------------------------------------------------------- keyboard-interactive

export interface AuthPrompt {
  requestId: number;
  host: string;
  port: number;
  user: string;
  name: string;
  instructions: string;
  /** `echo: false` means a secret (masked input). */
  fields: { text: string; echo: boolean }[];
}

/** Sends the answers (or `null` to cancel the login). */
export function respondAuthPrompt(requestId: number, answers: string[] | null): Promise<boolean> {
  return invoke("auth_prompt_respond", { requestId, answers });
}

// ---------------------------------------------------------------- file operations

export const sftpMkdir = (terminalId: TerminalId, dir: string, name: string) =>
  invoke<void>("sftp_mkdir", { terminalId, dir, name });

/** Renames within `dir`; never replaces an existing entry. */
export const sftpRename = (terminalId: TerminalId, dir: string, from: string, to: string) =>
  invoke<void>("sftp_rename", { terminalId, dir, from, to });

/** Deletes a file, symlink or empty folder. */
export const sftpDelete = (terminalId: TerminalId, dir: string, name: string) =>
  invoke<void>("sftp_delete", { terminalId, dir, name });

// ---------------------------------------------------------------- SSH keys

export interface KeyView {
  name: string;
  algorithm: string;
  fingerprint: string;
  comment: string;
  /** null when the private key file is missing or unreadable. */
  encrypted: boolean | null;
  hasPrivate: boolean;
}

export const listKeys = () => invoke<KeyView[]>("keys_list");

/** Generates `~/.ssh/<name>` (Ed25519); an empty passphrase leaves it unencrypted. */
export const generateKey = (name: string, comment: string, passphrase: string | null) =>
  invoke<KeyView>("key_generate", { name, comment, passphrase });

export const publicKeyText = (name: string) => invoke<string>("key_public", { name });

/** Adds the public key to authorized_keys on the server behind an SSH terminal. */
export const installKey = (terminalId: TerminalId, name: string) =>
  invoke<{ added: boolean; path: string }>("key_install", { terminalId, name });

// ---------------------------------------------------------------- telnet

/** Opens a saved Telnet session (unencrypted). */
export const telnetTerminal =
  (sessionId: string): Opener =>
  (cols, rows, onOutput, onExit) =>
    invoke<TerminalId>("telnet_open", { sessionId, cols, rows, ...channels(onOutput, onExit) });

/** Connects a saved session without a shell, for a file browser tab. */
export function openSshFiles(request: SshOpenRequest): Promise<{ id: TerminalId; label: string }> {
  return invoke("ssh_files_open", { request });
}

/** Opens an http(s) link from terminal output in the default browser. */
export const openWebLink = (url: string) => invoke<void>("open_web_link", { url });

// ---------------------------------------------------------------- crash reports

export interface CrashReport {
  /** File name; pass it back to dismiss the report. */
  id: string;
  kind: "backend" | "ui";
  /** Seconds since the Unix epoch. */
  time: number;
  summary: string;
  text: string;
}

export const crashReports = () => invoke<CrashReport[]>("crash_reports");
export const dismissCrashReports = (ids: string[]) => invoke<void>("crash_dismiss", { ids });
export const reportFrontendError = (message: string, details: string) =>
  invoke<void>("frontend_error", { message, details });

// ---------------------------------------------------------------- transfer history

export interface TransferRecord {
  id: number;
  direction: "download" | "upload";
  name: string;
  remotePath: string;
  localPath: string | null;
  size: number | null;
  outcome: "done" | "failed" | "cancelled";
  message: string | null;
  connection: string;
  /** Seconds since the Unix epoch. */
  finishedAt: number;
}

export const transferHistory = (limit = 200) => invoke<TransferRecord[]>("transfer_history", { limit });
export const clearTransferHistory = () => invoke<void>("transfer_history_clear");
